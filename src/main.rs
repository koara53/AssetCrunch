mod wav;
mod mesh;

use wgpu::*;
use wgpu::util::DeviceExt;
use std::time::Instant;

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("compress") => {
            let input  = args.get(2).expect("使い方: compress <input.wav> <output.gcwav>");
            let output = args.get(3).expect("使い方: compress <input.wav> <output.gcwav>");
            pollster::block_on(compress_wav(input, output));
        }
        Some("decompress") => {
            let input  = args.get(2).expect("使い方: decompress <input.gcwav> <output.wav>");
            let output = args.get(3).expect("使い方: decompress <input.gcwav> <output.wav>");
            decompress_wav(input, output);
        }
        Some("compress-mesh") => {
            let input  = args.get(2).expect("使い方: compress-mesh <input> <output.gcmesh>");
            let output = args.get(3).expect("使い方: compress-mesh <input> <output.gcmesh>");
            pollster::block_on(compress_mesh(input, output));
        }
        Some("decompress-mesh") => {
            let input  = args.get(2).expect("使い方: decompress-mesh <input.gcmesh> <output_dir>");
            let output = args.get(3).expect("使い方: decompress-mesh <input.gcmesh> <output_dir>");
            decompress_mesh(input, output);
        }
        Some("compress-text") => {
            let input  = args.get(2).expect("使い方: compress-text <input> <output.gcmesh>");
            let output = args.get(3).expect("使い方: compress-text <input> <output.gcmesh>");
            pollster::block_on(compress_mesh(input, output));
        }
        Some("decompress-folder") => {
            let input  = args.get(2).expect("使い方: decompress-folder <input_dir> <output_dir>");
            let output = args.get(3).expect("使い方: decompress-folder <input_dir> <output_dir>");
            decompress_folder(input, output);
        }
        Some("compress-folder") => {
            let input  = args.get(2).expect("使い方: compress-folder <input_dir> <output_dir>");
            let output = args.get(3).expect("使い方: compress-folder <input_dir> <output_dir>");
            pollster::block_on(compress_folder(input, output));
        }
        Some("--bench") => {
            pollster::block_on(run());
        }
        _ => {
            println!("AssetCrunch — GPU-accelerated game asset compressor");
            println!();
            println!("使い方:");
            println!("  assetcrunch compress         <input.wav>      <output.gcwav>");
            println!("  assetcrunch decompress       <input.gcwav>    <output.wav>");
            println!("  assetcrunch compress-mesh    <input.obj/fbx>  <output.gcmesh>");
            println!("  assetcrunch decompress-mesh  <input.gcmesh>   <output_dir>");
            println!("  assetcrunch compress-text    <input.json/txt> <output.gcmesh>");
            println!("  assetcrunch decompress-text  <input.gcmesh>   <output_dir>");
            println!("  assetcrunch compress-folder  <input_dir>      <output_dir>");
            println!();
            println!("対応フォーマット:");
            println!("  圧縮対象 : wav, obj, fbx, json, txt");
            println!("  スキップ : png, jpg, mp3, ogg (圧縮済みフォーマット)");
            println!("  assetcrunch decompress-folder <input_dir>      <output_dir>");
            println!();
            println!("オプション:");
            println!("  --bench  ベンチマークを実行");
        }
    }
}

#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
struct Params {
    total_bytes: u32,
    chunk_count: u32,
}

const CHUNK_SIZE:  u32 = 65536;
const OUT_STRIDE:  u32 = CHUNK_SIZE + 1024;
const SUB_COUNT:   u32 = 4;
const SUB_SIZE:    u32 = 16384;
const SUB_STRIDE:  u32 = SUB_SIZE + 256;
const BENCH_RUNS: usize = 5;

async fn setup_gpu() -> (Device, Queue, ComputePipeline, BindGroupLayout) {
    let instance = Instance::default();
    let adapter  = instance
        .request_adapter(&RequestAdapterOptions::default())
        .await.expect("アダプタ取得失敗");
    let (device, queue) = adapter
        .request_device(&DeviceDescriptor::default(), None)
        .await.expect("デバイス取得失敗");

    println!("GPU    : {}", adapter.get_info().name);
    println!("Backend: {:?}", adapter.get_info().backend);

    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("lz4"),
        source: ShaderSource::Wgsl(include_str!("lz4_compress.wgsl").into()),
    });

    let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            BindGroupLayoutEntry { binding: 0, visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer { ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false, min_binding_size: None }, count: None },
            BindGroupLayoutEntry { binding: 1, visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer { ty: BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false, min_binding_size: None }, count: None },
            BindGroupLayoutEntry { binding: 2, visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer { ty: BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false, min_binding_size: None }, count: None },
            BindGroupLayoutEntry { binding: 3, visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer { ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false, min_binding_size: None }, count: None },
        ],
    });

    let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: None,
        layout: Some(&device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None, bind_group_layouts: &[&bgl], push_constant_ranges: &[],
        })),
        module: &shader,
        entry_point: "main",
        compilation_options: Default::default(),
        cache: None,
    });

    (device, queue, pipeline, bgl)
}

// 戻り値: (sub_sizes, packed)
// sub_sizes: chunk_count × SUB_COUNT 個のサイズ
async fn gpu_compress_data(
    device: &Device,
    queue: &Queue,
    pipeline: &ComputePipeline,
    bgl: &BindGroupLayout,
    data: &[u8],
) -> (Vec<u32>, Vec<u8>) {
    let total_bytes = data.len() as u32;
    let chunk_count = total_bytes.div_ceil(CHUNK_SIZE);
    let sizes_count = chunk_count * SUB_COUNT;

    let src_buf = device.create_buffer_init(&util::BufferInitDescriptor {
        label: None, contents: data, usage: BufferUsages::STORAGE,
    });
    let dst_buf = device.create_buffer(&BufferDescriptor {
        label: None, size: (chunk_count * OUT_STRIDE) as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let sizes_buf = device.create_buffer(&BufferDescriptor {
        label: None, size: (sizes_count * 4) as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let params_buf = device.create_buffer_init(&util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::bytes_of(&Params { total_bytes, chunk_count }),
        usage: BufferUsages::UNIFORM,
    });
    let staging_sizes = device.create_buffer(&BufferDescriptor {
        label: None, size: (sizes_count * 4) as u64,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let staging_dst = device.create_buffer(&BufferDescriptor {
        label: None, size: (chunk_count * OUT_STRIDE) as u64,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bg = device.create_bind_group(&BindGroupDescriptor {
        label: None, layout: bgl,
        entries: &[
            BindGroupEntry { binding: 0, resource: src_buf.as_entire_binding() },
            BindGroupEntry { binding: 1, resource: dst_buf.as_entire_binding() },
            BindGroupEntry { binding: 2, resource: sizes_buf.as_entire_binding() },
            BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
        ],
    });

    let mut enc = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
    {
        let mut p = enc.begin_compute_pass(&ComputePassDescriptor {
            label: None, timestamp_writes: None,
        });
        p.set_pipeline(pipeline);
        p.set_bind_group(0, &bg, &[]);
        p.dispatch_workgroups(chunk_count, 1, 1);
    }
    enc.copy_buffer_to_buffer(&sizes_buf, 0, &staging_sizes, 0, (sizes_count * 4) as u64);
    enc.copy_buffer_to_buffer(&dst_buf,   0, &staging_dst,   0, (chunk_count * OUT_STRIDE) as u64);
    queue.submit(Some(enc.finish()));

    let (tx, rx) = std::sync::mpsc::channel();
    staging_sizes.slice(..).map_async(MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let sub_sizes: Vec<u32> = bytemuck::cast_slice(
        &staging_sizes.slice(..).get_mapped_range()
    ).to_vec();
    staging_sizes.unmap();

    let (tx2, rx2) = std::sync::mpsc::channel();
    staging_dst.slice(..).map_async(MapMode::Read, move |r| tx2.send(r).unwrap());
    device.poll(Maintain::Wait);
    rx2.recv().unwrap().unwrap();
    let dst_raw: Vec<u8> = bytemuck::cast_slice(
        &staging_dst.slice(..).get_mapped_range()
    ).to_vec();
    staging_dst.unmap();

    // サブチャンクを順番に詰める
    let mut packed = Vec::new();
    for ci in 0..chunk_count as usize {
        for si in 0..SUB_COUNT as usize {
            let sz    = sub_sizes[ci * SUB_COUNT as usize + si] as usize;
            let start = ci * OUT_STRIDE as usize + si * SUB_STRIDE as usize;
            packed.extend_from_slice(&dst_raw[start..start + sz]);
        }
    }

    (sub_sizes, packed)
}

async fn compress_wav(input: &str, output: &str) {
    let wav = match wav::WavFile::load(input) {
        Ok(w) => w,
        Err(e) => { eprintln!("エラー: {}", e); return; }
    };

    println!("\n入力: {}", input);
    println!("  {}ch  {}Hz  {}bit",
        wav.channels, wav.sample_rate, wav.bits_per_sample);
    println!("  PCMサイズ: {} bytes ({:.2} MB)",
        wav.pcm_data.len(), wav.pcm_data.len() as f64 / 1024.0 / 1024.0);

    let (device, queue, pipeline, bgl) = setup_gpu().await;

    println!("\nデルタ符号化中...");
    let pcm_to_compress = wav::delta_encode(
        &wav.pcm_data, wav.channels, wav.bits_per_sample
    );

    println!("GPU圧縮中...");
    let t = Instant::now();
    let (sub_sizes, packed) = gpu_compress_data(
        &device, &queue, &pipeline, &bgl, &pcm_to_compress
    ).await;
    let elapsed = t.elapsed();

    let chunk_count = (pcm_to_compress.len() as u32).div_ceil(CHUNK_SIZE) as usize;
    let total_compressed: u32 = sub_sizes.iter().sum();
    let original_size   = wav.header.len() + wav.pcm_data.len();
    let compressed_size = 8 + 4 + wav.header.len() + 4
        + sub_sizes.len() * 4 + packed.len();

    println!("完了: {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    println!("  PCM圧縮率: {:.2}%  ({} → {} bytes)",
        total_compressed as f64 / wav.pcm_data.len() as f64 * 100.0,
        wav.pcm_data.len(), total_compressed);
    println!("  ファイル削減: {:.2} MB → {:.2} MB  ({:.1}% 削減)",
        original_size as f64 / 1024.0 / 1024.0,
        compressed_size as f64 / 1024.0 / 1024.0,
        (1.0 - compressed_size as f64 / original_size as f64) * 100.0);

    let mut out_data = Vec::new();
    out_data.extend_from_slice(b"GCWAV001");
    out_data.extend_from_slice(&(wav.header.len() as u32).to_le_bytes());
    out_data.extend_from_slice(&wav.header);
    out_data.extend_from_slice(&(chunk_count as u32).to_le_bytes());
    for &sz in &sub_sizes {
        out_data.extend_from_slice(&sz.to_le_bytes());
    }
    out_data.extend_from_slice(&packed);

    std::fs::write(output, &out_data).expect("書き込み失敗");
    println!("出力: {}", output);
}

fn decompress_wav(input: &str, output: &str) {
    let data = std::fs::read(input).expect("読み込み失敗");
    if &data[0..8] != b"GCWAV001" {
        eprintln!("GCWAVファイルではありません");
        return;
    }

    let mut pos = 8usize;
    let header_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
    pos += 4;
    let header = data[pos..pos+header_len].to_vec();
    pos += header_len;

    let chunk_count = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
    pos += 4;

    let total_subs = chunk_count * SUB_COUNT as usize;
    let mut sub_sizes = Vec::with_capacity(total_subs);
    for _ in 0..total_subs {
        sub_sizes.push(u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize);
        pos += 4;
    }

    println!("解凍中... ({} チャンク × {} サブ)", chunk_count, SUB_COUNT);
    let t = Instant::now();

    let mut pcm_delta = Vec::new();
    for ci in 0..chunk_count {
        for si in 0..SUB_COUNT as usize {
            let sz = sub_sizes[ci * SUB_COUNT as usize + si];
            if sz == 0 { continue; }
            let chunk = &data[pos..pos + sz];
            let dec = lz4_flex::block::decompress(chunk, SUB_SIZE as usize)
                .expect("解凍失敗");
            pcm_delta.extend_from_slice(&dec);
            pos += sz;
        }
    }

    let channels        = u16::from_le_bytes(header[22..24].try_into().unwrap());
    let bits_per_sample = u16::from_le_bytes(header[34..36].try_into().unwrap());
    let pcm_data = wav::delta_decode(&pcm_delta, channels, bits_per_sample);

    println!("完了: {:.1}ms", t.elapsed().as_secs_f64() * 1000.0);
    let wav_bytes = wav::WavFile::rebuild(&header, &pcm_data);
    std::fs::write(output, &wav_bytes).expect("書き込み失敗");
    println!("出力: {}  ({:.2} MB)",
        output, wav_bytes.len() as f64 / 1024.0 / 1024.0);
}

async fn compress_mesh(input: &str, output: &str) {
    let mf = match mesh::MeshFile::load(input) {
        Ok(m) => m,
        Err(e) => { eprintln!("エラー: {}", e); return; }
    };

    let kind = match mf.ext.as_str() {
        "obj"  => "OBJ",
        "fbx"  => if mf.is_ascii_fbx { "FBX (ASCII)" } else { "FBX (Binary)" },
        "json" => "JSON",
        "txt"  => "TXT",
        _      => "BIN",
    };

    println!("\n入力: {}  [{}]", input, kind);
    println!("  サイズ: {} bytes ({:.2} MB)",
        mf.data.len(), mf.data.len() as f64 / 1024.0 / 1024.0);

    let (device, queue, pipeline, bgl) = setup_gpu().await;

    println!("GPU圧縮中...");
    let t = Instant::now();
    let (sub_sizes, packed) = gpu_compress_data(
        &device, &queue, &pipeline, &bgl, &mf.data
    ).await;
    let elapsed = t.elapsed();

    let chunk_count = (mf.data.len() as u32).div_ceil(CHUNK_SIZE) as usize;
    let total_compressed: u32 = sub_sizes.iter().sum();
    let original_size   = mf.data.len();
    let compressed_size = 8 + 1 + mf.ext.len() + 4 + sub_sizes.len() * 4 + packed.len();

    println!("完了: {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    println!("  圧縮率: {:.2}%  ({} → {} bytes)",
        total_compressed as f64 / original_size as f64 * 100.0,
        original_size, total_compressed);
    println!("  削減: {:.2} MB → {:.2} MB  ({:.1}% 削減)",
        original_size as f64 / 1024.0 / 1024.0,
        compressed_size as f64 / 1024.0 / 1024.0,
        (1.0 - compressed_size as f64 / original_size as f64) * 100.0);

    // .gcmesh フォーマット出力
    let mut out_data = Vec::new();
    out_data.extend_from_slice(b"GCMESH01");
    out_data.push(mf.ext.len() as u8);
    out_data.extend_from_slice(mf.ext.as_bytes());
    out_data.extend_from_slice(&(chunk_count as u32).to_le_bytes());
    for &sz in &sub_sizes {
        out_data.extend_from_slice(&sz.to_le_bytes());
    }
    out_data.extend_from_slice(&packed);
    std::fs::write(output, &out_data).expect("書き込み失敗");
    println!("出力: {}", output);
}

fn decompress_mesh(input: &str, output_dir: &str) {
    let data = std::fs::read(input).expect("読み込み失敗");
    if &data[0..8] != b"GCMESH01" {
        eprintln!("GCMESHファイルではありません");
        return;
    }

    let mut pos = 8usize;
    let ext_len = data[pos] as usize;
    pos += 1;
    let ext = String::from_utf8(data[pos..pos+ext_len].to_vec()).expect("ext読み込み失敗");
    pos += ext_len;

    let chunk_count = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
    pos += 4;

    let total_subs = chunk_count * SUB_COUNT as usize;
    let mut sub_sizes = Vec::with_capacity(total_subs);
    for _ in 0..total_subs {
        sub_sizes.push(u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize);
        pos += 4;
    }

    let t = Instant::now();
    let mut decompressed = Vec::new();
    for ci in 0..chunk_count {
        for si in 0..SUB_COUNT as usize {
            let sz = sub_sizes[ci * SUB_COUNT as usize + si];
            if sz == 0 { continue; }
            let chunk = &data[pos..pos + sz];
            let dec = lz4_flex::block::decompress(chunk, SUB_SIZE as usize)
                .expect("解凍失敗");
            decompressed.extend_from_slice(&dec);
            pos += sz;
        }
    }
    println!("完了: {:.1}ms", t.elapsed().as_secs_f64() * 1000.0);

    let stem = std::path::Path::new(input)
        .file_stem().unwrap().to_string_lossy();
    let out_path = std::path::Path::new(output_dir)
        .join(format!("{}.{}", stem, ext));
    std::fs::create_dir_all(output_dir).ok();
    std::fs::write(&out_path, &decompressed).expect("書き込み失敗");
    println!("出力: {}", out_path.display());
}

async fn compress_folder(input_dir: &str, output_dir: &str) {
    use std::path::Path;

    let out_path = Path::new(output_dir);
    std::fs::create_dir_all(out_path).expect("出力フォルダ作成失敗");

    let (device, queue, pipeline, bgl) = setup_gpu().await;

    let mut total_original   = 0u64;
    let mut total_compressed = 0u64;
    let mut total_copied     = 0u64;
    let mut results          = Vec::new();

    process_dir(
        Path::new(input_dir),
        out_path,
        &device, &queue, &pipeline, &bgl,
        &mut total_original,
        &mut total_compressed,
        &mut total_copied,
        &mut results,
    ).await;

    println!("\n========== 合計 ==========");
    println!("  圧縮対象 : {:.2} MB → {:.2} MB  ({:.1}% 削減)",
        total_original   as f64 / 1024.0 / 1024.0,
        total_compressed as f64 / 1024.0 / 1024.0,
       (1.0 - total_compressed as f64 / (total_original as f64).max(1.0)) * 100.0);
    println!("  コピー   : {:.2} MB（圧縮済みフォーマット等）",
        total_copied as f64 / 1024.0 / 1024.0);
    println!("  出力先   : {}", output_dir);

    let mut sorted = results.clone();
    sorted.sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap());
    if !sorted.is_empty() {
        println!("\n--- 圧縮率ランキング (効果が高い順) ---");
        for (name, _, _, ratio) in &sorted {
            println!("  {:.1}%  {}", ratio, name);
        }
    }
}
fn decompress_folder(input_dir: &str, output_dir: &str) {
    use std::path::Path;

    let out_path = Path::new(output_dir);
    std::fs::create_dir_all(out_path).expect("出力フォルダ作成失敗");

    let mut total_files    = 0u64;
    let mut total_restored = 0u64;
    let mut total_copied   = 0u64;

    decompress_dir(
        Path::new(input_dir),
        out_path,
        &mut total_files,
        &mut total_restored,
        &mut total_copied,
    );

    println!("\n========== 合計 ==========");
    println!("  解凍ファイル: {}個", total_files);
    println!("  解凍済み    : {:.2} MB", total_restored as f64 / 1024.0 / 1024.0);
    println!("  コピー      : {:.2} MB", total_copied   as f64 / 1024.0 / 1024.0);
    println!("  出力先      : {}", output_dir);
}

fn decompress_dir(
    input_dir:  &std::path::Path,
    output_dir: &std::path::Path,
    total_files:    &mut u64,
    total_restored: &mut u64,
    total_copied:   &mut u64,
) {
    let entries = match std::fs::read_dir(input_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let in_path  = entry.path();
        let filename = in_path.file_name().unwrap().to_string_lossy().to_string();

        if in_path.is_dir() {
            let sub_out = output_dir.join(&filename);
            std::fs::create_dir_all(&sub_out).ok();
            decompress_dir(&in_path, &sub_out, total_files, total_restored, total_copied);
            continue;
        }

        let ext = in_path.extension()
            .and_then(|x| x.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "gcwav" => {
                let data = match std::fs::read(&in_path) {
                    Ok(d) => d,
                    Err(e) => { println!("  スキップ: {} — {}", filename, e); continue; }
                };
                if &data[0..8] != b"GCWAV001" {
                    println!("  スキップ: {} — 不正なフォーマット", filename);
                    continue;
                }

                let mut pos = 8usize;
                let header_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
                pos += 4;
                let header = data[pos..pos+header_len].to_vec();
                pos += header_len;
                let chunk_count = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
                pos += 4;

                let total_subs = chunk_count * SUB_COUNT as usize;
                let mut sub_sizes = Vec::with_capacity(total_subs);
                for _ in 0..total_subs {
                    sub_sizes.push(u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize);
                    pos += 4;
                }

                let mut pcm_delta = Vec::new();
                for ci in 0..chunk_count {
                    for si in 0..SUB_COUNT as usize {
                        let sz = sub_sizes[ci * SUB_COUNT as usize + si];
                        if sz == 0 { continue; }
                        let chunk = &data[pos..pos + sz];
                        match lz4_flex::block::decompress(chunk, SUB_SIZE as usize) {
                            Ok(dec) => pcm_delta.extend_from_slice(&dec),
                            Err(e) => {
                                println!("  解凍失敗: {} — {}", filename, e);
                                break;
                            }
                        }
                        pos += sz;
                    }
                }

                let channels        = u16::from_le_bytes(header[22..24].try_into().unwrap());
                let bits_per_sample = u16::from_le_bytes(header[34..36].try_into().unwrap());
                let pcm_data = wav::delta_decode(&pcm_delta, channels, bits_per_sample);
                let wav_bytes = wav::WavFile::rebuild(&header, &pcm_data);

                let out_file = output_dir.join(
                    format!("{}.wav", in_path.file_stem().unwrap().to_string_lossy())
                );
                let size = wav_bytes.len() as u64;
                std::fs::write(&out_file, &wav_bytes).expect("書き込み失敗");

                println!("  [WAV ] {:.<50} {:.1}KB", filename, size as f64 / 1024.0);
                *total_files    += 1;
                *total_restored += size;
            }

            "gcmesh" => {
                let data = match std::fs::read(&in_path) {
                    Ok(d) => d,
                    Err(e) => { println!("  スキップ: {} — {}", filename, e); continue; }
                };
                if &data[0..8] != b"GCMESH01" {
                    println!("  スキップ: {} — 不正なフォーマット", filename);
                    continue;
                }

                let mut pos = 8usize;
                let ext_len = data[pos] as usize;
                pos += 1;
                let orig_ext = String::from_utf8(data[pos..pos+ext_len].to_vec())
                    .unwrap_or("bin".into());
                pos += ext_len;
                let chunk_count = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
                pos += 4;

                let total_subs = chunk_count * SUB_COUNT as usize;
                let mut sub_sizes = Vec::with_capacity(total_subs);
                for _ in 0..total_subs {
                    sub_sizes.push(u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize);
                    pos += 4;
                }

                let mut decompressed = Vec::new();
                for ci in 0..chunk_count {
                    for si in 0..SUB_COUNT as usize {
                        let sz = sub_sizes[ci * SUB_COUNT as usize + si];
                        if sz == 0 { continue; }
                        let chunk = &data[pos..pos + sz];
                        match lz4_flex::block::decompress(chunk, SUB_SIZE as usize) {
                            Ok(dec) => decompressed.extend_from_slice(&dec),
                            Err(e) => {
                                println!("  解凍失敗: {} — {}", filename, e);
                                break;
                            }
                        }
                        pos += sz;
                    }
                }

                let out_file = output_dir.join(
                    format!("{}.{}",
                        in_path.file_stem().unwrap().to_string_lossy(),
                        orig_ext)
                );
                let size = decompressed.len() as u64;
                std::fs::write(&out_file, &decompressed).expect("書き込み失敗");

                println!("  [MESH] {:.<50} {:.1}KB", filename, size as f64 / 1024.0);
                *total_files    += 1;
                *total_restored += size;
            }

            _ => {
                // gcwav/gcmesh以外はそのままコピー
                let out_file = output_dir.join(&filename);
                let size = in_path.metadata().map(|m| m.len()).unwrap_or(0);
                std::fs::copy(&in_path, &out_file).ok();
                *total_copied += size;
            }
        }
    }
}

fn process_dir<'a>(
    input_dir: &'a std::path::Path,
    output_dir: &'a std::path::Path,
    device: &'a Device,
    queue: &'a Queue,
    pipeline: &'a ComputePipeline,
    bgl: &'a BindGroupLayout,
    total_original:   &'a mut u64,
    total_compressed: &'a mut u64,
    total_copied:     &'a mut u64,
    results: &'a mut Vec<(String, usize, usize, f64)>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>> {
    Box::pin(async move {
        let entries = match std::fs::read_dir(input_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let in_path  = entry.path();
            let filename = in_path.file_name().unwrap().to_string_lossy().to_string();

            if in_path.is_dir() {
                let sub_out = output_dir.join(&filename);
                std::fs::create_dir_all(&sub_out).ok();
                process_dir(
                    &in_path, &sub_out,
                    device, queue, pipeline, bgl,
                    total_original, total_compressed, total_copied,
                    results,
                ).await;
                continue;
            }

            let ext = in_path.extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_lowercase();

            match ext.as_str() {
                "wav" => {
                    let wav = match wav::WavFile::load(in_path.to_str().unwrap()) {
                        Ok(w) => w,
                        Err(e) => {
                            println!("  スキップ: {} — {}", filename, e);
                            let out_file = output_dir.join(&filename);
                            std::fs::copy(&in_path, &out_file).ok();
                            continue;
                        }
                    };
                    let original_size = wav.header.len() + wav.pcm_data.len();
                    let pcm_enc = wav::delta_encode(
                        &wav.pcm_data, wav.channels, wav.bits_per_sample
                    );
                    let (sub_sizes, packed) = gpu_compress_data(
                        device, queue, pipeline, bgl, &pcm_enc
                    ).await;
                    let chunk_count = (pcm_enc.len() as u32).div_ceil(CHUNK_SIZE) as usize;
                    let compressed_size = 8 + 4 + wav.header.len() + 4
                        + sub_sizes.len() * 4 + packed.len();

                    let out_file = output_dir.join(
                        format!("{}.gcwav",
                            in_path.file_stem().unwrap().to_string_lossy())
                    );
                    let mut out_data = Vec::new();
                    out_data.extend_from_slice(b"GCWAV001");
                    out_data.extend_from_slice(&(wav.header.len() as u32).to_le_bytes());
                    out_data.extend_from_slice(&wav.header);
                    out_data.extend_from_slice(&(chunk_count as u32).to_le_bytes());
                    for &sz in &sub_sizes { out_data.extend_from_slice(&sz.to_le_bytes()); }
                    out_data.extend_from_slice(&packed);
                    std::fs::write(&out_file, &out_data).expect("書き込み失敗");

                    let ratio = compressed_size as f64 / original_size as f64 * 100.0;
                    println!("  [WAV ] {:.<50} {:>7.1}KB → {:>7.1}KB  ({:+.1}%)",
                        filename,
                        original_size   as f64 / 1024.0,
                        compressed_size as f64 / 1024.0,
                        ratio - 100.0);
                    *total_original   += original_size as u64;
                    *total_compressed += compressed_size as u64;
                    results.push((filename, original_size, compressed_size, ratio));
                }

                "obj" | "fbx" | "json" | "txt" => {
                    let mf = match mesh::MeshFile::load(in_path.to_str().unwrap()) {
                        Ok(m) => m,
                        Err(e) => {
                            println!("  スキップ: {} — {}", filename, e);
                            let out_file = output_dir.join(&filename);
                            std::fs::copy(&in_path, &out_file).ok();
                            continue;
                        }
                    };
                    let original_size = mf.data.len();
                    let (sub_sizes, packed) = gpu_compress_data(
                        device, queue, pipeline, bgl, &mf.data
                    ).await;
                    let chunk_count = (mf.data.len() as u32).div_ceil(CHUNK_SIZE) as usize;
                    let compressed_size = 8 + 1 + mf.ext.len() + 4
                        + sub_sizes.len() * 4 + packed.len();

                    let out_file = output_dir.join(
                        format!("{}.gcmesh",
                            in_path.file_stem().unwrap().to_string_lossy())
                    );
                    let mut out_data = Vec::new();
                    out_data.extend_from_slice(b"GCMESH01");
                    out_data.push(mf.ext.len() as u8);
                    out_data.extend_from_slice(mf.ext.as_bytes());
                    out_data.extend_from_slice(&(chunk_count as u32).to_le_bytes());
                    for &sz in &sub_sizes { out_data.extend_from_slice(&sz.to_le_bytes()); }
                    out_data.extend_from_slice(&packed);
                    std::fs::write(&out_file, &out_data).expect("書き込み失敗");

                    let kind = match ext.as_str() {
                        "obj"  => "OBJ  ",
                        "fbx"  => if mf.is_ascii_fbx { "FBX/A" } else { "FBX/B" },
                        "json" => "JSON ",
                        "txt"  => "TXT  ",
                        _      => "BIN  ",
                    };

                    let ratio = compressed_size as f64 / original_size as f64 * 100.0;
                    println!("  [{}] {:.<50} {:>7.1}KB → {:>7.1}KB  ({:+.1}%)",
                        kind, filename,
                        original_size   as f64 / 1024.0,
                        compressed_size as f64 / 1024.0,
                        ratio - 100.0);
                    *total_original   += original_size as u64;
                    *total_compressed += compressed_size as u64;
                    results.push((filename, original_size, compressed_size, ratio));
                }

                _ => {
                    let out_file = output_dir.join(&filename);
                    let size = in_path.metadata().map(|m| m.len()).unwrap_or(0);
                    std::fs::copy(&in_path, &out_file).ok();
                    println!("  [COPY ] {:.<50} {:>7.1}KB",
                        filename, size as f64 / 1024.0);
                    *total_copied += size;
                }
            }
        }
    })
}

fn gen_random(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    let mut x: u32 = 0xDEADBEEF;
    for _ in 0..size {
        x ^= x << 13; x ^= x >> 17; x ^= x << 5;
        data.push((x & 0xFF) as u8);
    }
    data
}

fn gen_texture_like(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    let mut x: u32 = 0xDEADBEEF;
    let mut i = 0;
    while i < size {
        x ^= x << 13; x ^= x >> 17; x ^= x << 5;
        let base = (x & 0xFF) as u8;
        let block = (32 + (x >> 8 & 95)) as usize;
        for j in 0..block.min(size - i) {
            let noise = if j % 16 == 0 { ((x >> 16) & 0x07) as u8 } else { 0 };
            data.push(base.wrapping_add(noise));
        }
        i += block;
    }
    data
}

fn gen_audio_like(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    let mut x: u32 = 0xDEADBEEF;
    for i in 0..size {
        x ^= x << 13; x ^= x >> 17; x ^= x << 5;
        let wave = ((i % 64) as f32 / 64.0 * std::f32::consts::TAU).sin();
        let noise = ((x & 0x0F) as i8 - 8) as f32 / 64.0;
        let sample = ((wave + noise) * 120.0 + 128.0).clamp(0.0, 255.0) as u8;
        data.push(sample);
    }
    data
}

async fn run() {
    let (device, queue, pipeline, bgl) = setup_gpu().await;
    println!();

    let datasets: &[(&str, fn(usize) -> Vec<u8>)] = &[
        ("乱数 (最悪ケース)", gen_random),
        ("テクスチャ風",      gen_texture_like),
        ("音声波形風",        gen_audio_like),
    ];

    let test_mb = 64;

    for (name, gen) in datasets {
        let input_data  = gen(test_mb * 1024 * 1024);
        let total_bytes = input_data.len() as u32;
        let chunk_count = total_bytes.div_ceil(CHUNK_SIZE);

        println!("========== {} — {}MB ==========", name, test_mb);

        let src_buf = device.create_buffer_init(&util::BufferInitDescriptor {
            label: None, contents: &input_data, usage: BufferUsages::STORAGE,
        });
        let dst_buf = device.create_buffer(&BufferDescriptor {
            label: None, size: (chunk_count * OUT_STRIDE) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let sizes_count = chunk_count * SUB_COUNT;
        let sizes_buf = device.create_buffer(&BufferDescriptor {
            label: None, size: (sizes_count * 4) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let params_buf = device.create_buffer_init(&util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&Params { total_bytes, chunk_count }),
            usage: BufferUsages::UNIFORM,
        });
        let staging_sizes = device.create_buffer(&BufferDescriptor {
            label: None, size: (sizes_count * 4) as u64,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg = device.create_bind_group(&BindGroupDescriptor {
            label: None, layout: &bgl,
            entries: &[
                BindGroupEntry { binding: 0, resource: src_buf.as_entire_binding() },
                BindGroupEntry { binding: 1, resource: dst_buf.as_entire_binding() },
                BindGroupEntry { binding: 2, resource: sizes_buf.as_entire_binding() },
                BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
            ],
        });

        // ウォームアップ
        {
            let mut enc = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
            { let mut p = enc.begin_compute_pass(&ComputePassDescriptor { label: None, timestamp_writes: None });
              p.set_pipeline(&pipeline); p.set_bind_group(0, &bg, &[]); p.dispatch_workgroups(chunk_count, 1, 1); }
            queue.submit(Some(enc.finish()));
            device.poll(Maintain::Wait);
        }

        let mut gpu_times = Vec::with_capacity(BENCH_RUNS);
        for _ in 0..BENCH_RUNS {
            let mut enc = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
            { let mut p = enc.begin_compute_pass(&ComputePassDescriptor { label: None, timestamp_writes: None });
              p.set_pipeline(&pipeline); p.set_bind_group(0, &bg, &[]); p.dispatch_workgroups(chunk_count, 1, 1); }
            enc.copy_buffer_to_buffer(&sizes_buf, 0, &staging_sizes, 0, (sizes_count * 4) as u64);
            let t = Instant::now();
            queue.submit(Some(enc.finish()));
            let (tx, rx) = std::sync::mpsc::channel();
            staging_sizes.slice(..).map_async(MapMode::Read, move |r| tx.send(r).unwrap());
            device.poll(Maintain::Wait);
            rx.recv().unwrap().unwrap();
            gpu_times.push(t.elapsed().as_secs_f64());
            staging_sizes.unmap();
        }

        // 圧縮サイズ取得
        {
            let mut enc = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
            { let mut p = enc.begin_compute_pass(&ComputePassDescriptor { label: None, timestamp_writes: None });
              p.set_pipeline(&pipeline); p.set_bind_group(0, &bg, &[]); p.dispatch_workgroups(chunk_count, 1, 1); }
            enc.copy_buffer_to_buffer(&sizes_buf, 0, &staging_sizes, 0, (sizes_count * 4) as u64);
            queue.submit(Some(enc.finish()));
            device.poll(Maintain::Wait);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        staging_sizes.slice(..).map_async(MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(Maintain::Wait);
        rx.recv().unwrap().unwrap();
        let sub_sizes: Vec<u32> = bytemuck::cast_slice(
            &staging_sizes.slice(..).get_mapped_range()
        ).to_vec();
        staging_sizes.unmap();
        let gpu_bytes: u32 = sub_sizes.iter().sum();

        let gpu_avg = gpu_times.iter().sum::<f64>() / BENCH_RUNS as f64;
        let gpu_tp  = test_mb as f64 / gpu_avg;

        println!("[GPU]");
        println!("  avg: {:.1}ms  throughput: {:.0} MB/s  圧縮率: {:.2}%",
            gpu_avg * 1000.0, gpu_tp,
            gpu_bytes as f64 / input_data.len() as f64 * 100.0);

        let mut cpu_times = Vec::with_capacity(BENCH_RUNS);
        let mut cpu_bytes = 0usize;
        for run in 0..BENCH_RUNS {
            let t = Instant::now();
            let mut total = 0;
            for ci in 0..chunk_count as usize {
                let s = ci * CHUNK_SIZE as usize;
                let e = (s + CHUNK_SIZE as usize).min(input_data.len());
                total += lz4_flex::block::compress(&input_data[s..e]).len();
            }
            cpu_times.push(t.elapsed().as_secs_f64());
            if run == BENCH_RUNS - 1 { cpu_bytes = total; }
        }

        let cpu_avg = cpu_times.iter().sum::<f64>() / BENCH_RUNS as f64;
        let cpu_tp  = test_mb as f64 / cpu_avg;

        println!("[CPU シングルスレッド]");
        println!("  avg: {:.1}ms  throughput: {:.0} MB/s  圧縮率: {:.2}%",
            cpu_avg * 1000.0, cpu_tp,
            cpu_bytes as f64 / input_data.len() as f64 * 100.0);

        println!("[比較] GPU speedup: {:.2}x\n", gpu_tp / cpu_tp);
    }
}
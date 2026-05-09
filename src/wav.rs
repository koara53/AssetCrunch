use std::fs;

pub struct WavFile {
    pub header: Vec<u8>,
    pub pcm_data: Vec<u8>,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
}

impl WavFile {
    pub fn load(path: &str) -> Result<Self, String> {
        let raw = fs::read(path).map_err(|e| e.to_string())?;
        if raw.len() < 44 {
            return Err("ファイルが短すぎます".into());
        }
        if &raw[0..4] != b"RIFF" || &raw[8..12] != b"WAVE" {
            return Err("WAVファイルではありません".into());
        }

        let channels        = u16::from_le_bytes([raw[22], raw[23]]);
        let sample_rate     = u32::from_le_bytes([raw[24], raw[25], raw[26], raw[27]]);
        let bits_per_sample = u16::from_le_bytes([raw[34], raw[35]]);

        let mut pos = 12usize;
        loop {
            if pos + 8 > raw.len() {
                return Err("dataチャンクが見つかりません".into());
            }
            let chunk_size = u32::from_le_bytes(
                raw[pos+4..pos+8].try_into().unwrap()
            ) as usize;
            if &raw[pos..pos+4] == b"data" {
                let pcm_start = pos + 8;
                let pcm_len   = chunk_size.min(raw.len() - pcm_start);
                return Ok(Self {
                    header:          raw[..pcm_start].to_vec(),
                    pcm_data:        raw[pcm_start..pcm_start + pcm_len].to_vec(),
                    channels,
                    sample_rate,
                    bits_per_sample,
                });
            }
            if chunk_size == 0 {
                return Err("不正なチャンクサイズ".into());
            }
            pos += 8 + chunk_size;
        }
    }

    pub fn rebuild(header: &[u8], pcm_data: &[u8]) -> Vec<u8> {
        let mut out = header.to_vec();
        let riff_size = (out.len() + pcm_data.len() - 8) as u32;
        out[4..8].copy_from_slice(&riff_size.to_le_bytes());
        let hl = out.len();
        out[hl-4..hl].copy_from_slice(&(pcm_data.len() as u32).to_le_bytes());
        out.extend_from_slice(pcm_data);
        out
    }
}

pub fn delta_encode(pcm: &[u8], _channels: u16, bits_per_sample: u16) -> Vec<u8> {
    let bytes_per_sample = (bits_per_sample / 8) as usize;
    let sample_count = pcm.len() / bytes_per_sample;

    match bits_per_sample {
        16 => {
            // 16bit: デルタ符号化 + バイトプレーン分離
            let mut deltas = vec![0i16; sample_count];
            let mut prev = 0i16;
            for i in 0..sample_count {
                let pos = i * 2;
                let cur = i16::from_le_bytes(pcm[pos..pos+2].try_into().unwrap());
                deltas[i] = cur.wrapping_sub(prev);
                prev = cur;
            }
            // バイトプレーン分離: 下位バイト列 → 上位バイト列
            let mut out = Vec::with_capacity(pcm.len());
            for i in 0..sample_count {
                out.push((deltas[i] & 0xFF) as u8);
            }
            for i in 0..sample_count {
                out.push(((deltas[i] >> 8) & 0xFF) as u8);
            }
            out
        }
        32 => {
            // 32bit float: バイトプレーン分離のみ
            // 指数部・仮数部の上位ビットが集まって繰り返しが生まれる
            let mut planes = vec![Vec::with_capacity(sample_count); 4];
            for i in 0..sample_count {
                let pos = i * 4;
                planes[0].push(pcm[pos]);
                planes[1].push(pcm[pos+1]);
                planes[2].push(pcm[pos+2]);
                planes[3].push(pcm[pos+3]);
            }
            // plane2・plane3 (指数部〜仮数部上位) にデルタをかける
            for p in 2..4 {
                let mut prev = 0u8;
                for v in planes[p].iter_mut() {
                    let cur = *v;
                    *v = cur.wrapping_sub(prev);
                    prev = cur;
                }
            }
            let mut out = Vec::with_capacity(pcm.len());
            for plane in &planes {
                out.extend_from_slice(plane);
            }
            out
        }
        _ => pcm.to_vec()
    }
}

pub fn delta_decode(pcm: &[u8], _channels: u16, bits_per_sample: u16) -> Vec<u8> {
    let bytes_per_sample = (bits_per_sample / 8) as usize;
    let sample_count = pcm.len() / bytes_per_sample;

    match bits_per_sample {
        16 => {
            let half = sample_count;
            let lo = &pcm[..half];
            let hi = &pcm[half..half*2];
            // デルタ復号
            let mut out = Vec::with_capacity(pcm.len());
            let mut acc = 0i16;
            for i in 0..sample_count {
                let delta = (lo[i] as i16) | ((hi[i] as i16) << 8);
                acc = acc.wrapping_add(delta);
                out.extend_from_slice(&acc.to_le_bytes());
            }
            out
        }
        32 => {
            let n = sample_count;
            let mut planes: Vec<Vec<u8>> = vec![
                pcm[0..n].to_vec(),
                pcm[n..n*2].to_vec(),
                pcm[n*2..n*3].to_vec(),
                pcm[n*3..n*4].to_vec(),
            ];
            // plane2・plane3 のデルタ復号
            for p in 2..4 {
                let mut acc = 0u8;
                for v in planes[p].iter_mut() {
                    acc = acc.wrapping_add(*v);
                    *v = acc;
                }
            }
            // インターリーブに戻す
            let mut out = Vec::with_capacity(pcm.len());
            for i in 0..n {
                out.push(planes[0][i]);
                out.push(planes[1][i]);
                out.push(planes[2][i]);
                out.push(planes[3][i]);
            }
            out
        }
        _ => pcm.to_vec()
    }
}
const CHUNK_SIZE:    u32 = 65536u;
const SUB_COUNT:     u32 = 4u;
const SUB_SIZE:      u32 = 16384u;  // 64KB / 4
const OUT_STRIDE:    u32 = CHUNK_SIZE + 1024u;
const SUB_STRIDE:    u32 = SUB_SIZE + 256u;  // サブチャンクごとの出力余裕
const HASH_BITS:     u32 = 10u;
const HASH_PER_SUB:  u32 = 1024u;  // 2^10 = 1024エントリ × 4サブ = 4096合計
const THREADS:       u32 = 256u;
const THREADS_PER_SUB: u32 = 64u;
const NO_ENTRY:      u32 = 0xFFFFFFFFu;
const MIN_MATCH:     u32 = 4u;

@group(0) @binding(0) var<storage, read>       src:       array<u32>;
@group(0) @binding(1) var<storage, read_write> dst:       array<u32>;
@group(0) @binding(2) var<storage, read_write> dst_sizes: array<u32>;

struct Params { total_bytes: u32, chunk_count: u32 }
@group(0) @binding(3) var<uniform> params: Params;

var<workgroup> hash_tbl: array<u32, 4096>;

fn lz4_hash4(val: u32) -> u32 {
    return (val * 2654435761u) >> (32u - HASH_BITS);
}

fn src_byte(pos: u32) -> u32 {
    return (src[pos >> 2u] >> ((pos & 3u) * 8u)) & 0xFFu;
}

fn src_u32(pos: u32) -> u32 {
    let wi  = pos >> 2u;
    let off = pos & 3u;
    if (off == 0u) { return src[wi]; }
    return (src[wi] >> (off * 8u)) | (src[wi + 1u] << ((4u - off) * 8u));
}

fn dst_write(pos: u32, val: u32) {
    let wi = pos >> 2u;
    let sh = (pos & 3u) * 8u;
    dst[wi] = (dst[wi] & ~(0xFFu << sh)) | ((val & 0xFFu) << sh);
}

fn dst_write_u16(pos: u32, val: u32) {
    dst_write(pos,      val & 0xFFu);
    dst_write(pos + 1u, (val >> 8u) & 0xFFu);
}

fn count_match_extra(pos: u32, mpos: u32, sub_end: u32) -> u32 {
    if (sub_end <= pos + MIN_MATCH) { return 0u; }
    let max_extra = min(sub_end - pos - MIN_MATCH, 65534u);
    var extra: u32 = 0u;
    loop {
        if (extra >= max_extra) { break; }
        if (src_byte(pos + MIN_MATCH + extra) != src_byte(mpos + MIN_MATCH + extra)) { break; }
        extra += 1u;
    }
    return extra;
}

fn emit_seq(
    op: ptr<function, u32>,
    lit_start: u32, lit_end: u32,
    match_off: u32, extra_match: u32,
) {
    let lc = lit_end - lit_start;
    dst_write(*op, (min(lc, 15u) << 4u) | min(extra_match, 15u));
    *op += 1u;

    if (lc >= 15u) {
        var r = lc - 15u;
        loop {
            if (r < 255u) { break; }
            dst_write(*op, 255u); *op += 1u;
            r -= 255u;
        }
        dst_write(*op, r); *op += 1u;
    }

    for (var i = 0u; i < lc; i += 1u) {
        dst_write(*op, src_byte(lit_start + i));
        *op += 1u;
    }

    dst_write_u16(*op, match_off); *op += 2u;

    if (extra_match >= 15u) {
        var r = extra_match - 15u;
        loop {
            if (r < 255u) { break; }
            dst_write(*op, 255u); *op += 1u;
            r -= 255u;
        }
        dst_write(*op, r); *op += 1u;
    }
}

fn emit_final(op: ptr<function, u32>, lit_start: u32, lit_end: u32) {
    let lc = lit_end - lit_start;
    dst_write(*op, min(lc, 15u) << 4u); *op += 1u;

    if (lc >= 15u) {
        var r = lc - 15u;
        loop {
            if (r < 255u) { break; }
            dst_write(*op, 255u); *op += 1u;
            r -= 255u;
        }
        dst_write(*op, r); *op += 1u;
    }

    for (var i = 0u; i < lc; i += 1u) {
        dst_write(*op, src_byte(lit_start + i));
        *op += 1u;
    }
}

// サブチャンクLZ4圧縮 (各リードスレッドが呼ぶ)
fn compress_sub(
    sub_id:      u32,
    chunk_start: u32,
    chunk_end:   u32,
    out_base:    u32,
) -> u32 {
    let sub_start = chunk_start + sub_id * SUB_SIZE;
    let sub_end   = min(sub_start + SUB_SIZE, chunk_end);

    if (sub_start >= chunk_end) { return 0u; }

    let hash_base = sub_id * HASH_PER_SUB;
    var op        = out_base + sub_id * SUB_STRIDE;
    let op_base   = op;
    var pos       = sub_start;
    var lit_run   = pos;

    if (sub_end < sub_start + MIN_MATCH + 12u) {
        emit_final(&op, lit_run, sub_end);
        return op - op_base;
    }

    let safe_end = sub_end - 12u;

    loop {
        if (pos >= safe_end) { break; }

        let word = src_u32(pos);
        let h    = lz4_hash4(word);
        let hi   = hash_base + (h & (HASH_PER_SUB - 1u));
        let prev = hash_tbl[hi];
        hash_tbl[hi] = pos;

        let off = pos - prev;
        if (prev != NO_ENTRY && prev >= sub_start && off <= 65535u) {
            if (src_u32(prev) == word) {
                let extra = count_match_extra(pos, prev, sub_end);
                emit_seq(&op, lit_run, pos, off, extra);
                pos    += MIN_MATCH + extra;
                lit_run = pos;
                continue;
            }
        }
        pos += 1u;
    }

    emit_final(&op, lit_run, sub_end);
    return op - op_base;
}

@compute @workgroup_size(256, 1, 1)
fn main(
    @builtin(workgroup_id)        wid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let chunk_id    = wid.x;
    let chunk_start = chunk_id * CHUNK_SIZE;
    let chunk_end   = min(chunk_start + CHUNK_SIZE, params.total_bytes);
    let sub_id      = lid.x / THREADS_PER_SUB;  // 0~3
    let local_id    = lid.x % THREADS_PER_SUB;  // 0~63

    // Phase 1: 全スレッドでハッシュテーブルをクリア
    for (var i = lid.x; i < 4096u; i += THREADS) {
        hash_tbl[i] = NO_ENTRY;
    }
    workgroupBarrier();

    // Phase 2: 各サブチャンクのリードスレッド(local_id==0)が圧縮
    let out_base = chunk_id * OUT_STRIDE;
    if (local_id == 0u) {
        let sz = compress_sub(sub_id, chunk_start, chunk_end, out_base);

        // サブチャンクごとのサイズをdst_sizesに書き込む
        // インデックス: chunk_id * SUB_COUNT + sub_id
        dst_sizes[chunk_id * SUB_COUNT + sub_id] = sz;
    }
}
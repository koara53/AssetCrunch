const CHUNK_SIZE: u32 = 65536u;
const OUT_STRIDE: u32 = CHUNK_SIZE + 1024u;  // ワーストケース余裕込み
const HASH_BITS: u32 = 13u;
const HASH_SIZE: u32 = 8192u;
const THREADS:    u32 = 256u;
const NO_ENTRY:   u32 = 0xFFFFFFFFu;
const MIN_MATCH:  u32 = 4u;

@group(0) @binding(0) var<storage, read>       src:       array<u32>;
@group(0) @binding(1) var<storage, read_write> dst:       array<u32>;
@group(0) @binding(2) var<storage, read_write> dst_sizes: array<u32>;

struct Params { total_bytes: u32, chunk_count: u32 }
@group(0) @binding(3) var<uniform> params: Params;

var<workgroup> hash_tbl: array<u32, 4096>;

fn lz4_hash4(val: u32) -> u32 {
    return (val * 2654435761u) >> (32u - HASH_BITS);
}

// バイト単位読み取り
fn src_byte(pos: u32) -> u32 {
    return (src[pos >> 2u] >> ((pos & 3u) * 8u)) & 0xFFu;
}

// 非アライン対応 u32 読み取り
fn src_u32(pos: u32) -> u32 {
    let wi  = pos >> 2u;
    let off = pos & 3u;
    if (off == 0u) { return src[wi]; }
    return (src[wi] >> (off * 8u)) | (src[wi + 1u] << ((4u - off) * 8u));
}

// バイト単位書き込み
fn dst_write(pos: u32, val: u32) {
    let wi = pos >> 2u;
    let sh = (pos & 3u) * 8u;
    dst[wi] = (dst[wi] & ~(0xFFu << sh)) | ((val & 0xFFu) << sh);
}

// u16 リトルエンディアン書き込み
fn dst_write_u16(pos: u32, val: u32) {
    dst_write(pos,      val & 0xFFu);
    dst_write(pos + 1u, (val >> 8u) & 0xFFu);
}

// MIN_MATCH 超過のマッチ長をカウント
fn count_match_extra(pos: u32, mpos: u32, chunk_end: u32) -> u32 {
    if (chunk_end <= pos + MIN_MATCH) { return 0u; }
    let max_extra = min(chunk_end - pos - MIN_MATCH, 65534u);
    var extra: u32 = 0u;
    loop {
        if (extra >= max_extra) { break; }
        if (src_byte(pos + MIN_MATCH + extra) != src_byte(mpos + MIN_MATCH + extra)) { break; }
        extra += 1u;
    }
    return extra;
}

// LZ4シーケンス出力（リテラル列 + マッチ）
fn emit_seq(
    op: ptr<function, u32>,
    lit_start: u32, lit_end: u32,
    match_off: u32, extra_match: u32,
) {
    let lc = lit_end - lit_start;

    // トークンバイト: [リテラル長nibble | マッチ長nibble]
    dst_write(*op, (min(lc, 15u) << 4u) | min(extra_match, 15u));
    *op += 1u;

    // リテラル長 延長バイト
    if (lc >= 15u) {
        var r = lc - 15u;
        loop {
            if (r < 255u) { break; }
            dst_write(*op, 255u); *op += 1u;
            r -= 255u;
        }
        dst_write(*op, r); *op += 1u;
    }

    // リテラルバイト列
    for (var i = 0u; i < lc; i += 1u) {
        dst_write(*op, src_byte(lit_start + i));
        *op += 1u;
    }

    // マッチオフセット (2バイト LE)
    dst_write_u16(*op, match_off); *op += 2u;

    // マッチ長 延長バイト
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

// 末尾リテラル専用出力（マッチなし・ブロック末尾）
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

@compute @workgroup_size(256, 1, 1)
fn main(
    @builtin(workgroup_id)        wid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let chunk_id    = wid.x;
    let chunk_start = chunk_id * CHUNK_SIZE;
    let chunk_end   = min(chunk_start + CHUNK_SIZE, params.total_bytes);

    // Phase 1: 全スレッドでハッシュテーブルをクリア (各16エントリ担当)
    for (var i = lid.x; i < HASH_SIZE; i += THREADS) {
        hash_tbl[i] = NO_ENTRY;
    }
    workgroupBarrier();

    if (lid.x != 0u) { return; }

    var op      = chunk_id * OUT_STRIDE;
    let op_base = op;
    var pos     = chunk_start;
    var lit_run = pos;

    // チャンクが小さすぎる場合はリテラルのみ
    if (chunk_end < chunk_start + MIN_MATCH + 12u) {
        emit_final(&op, lit_run, chunk_end);
        dst_sizes[chunk_id] = op - op_base;
        return;
    }

    // LZ4末尾制約: 最後12バイトはマッチ禁止
    let safe_end = chunk_end - 12u;

    loop {
        if (pos >= safe_end) { break; }

        let word = src_u32(pos);
        let h    = lz4_hash4(word);
        let prev = hash_tbl[h];
        hash_tbl[h] = pos;

        let off = pos - prev;
        if (prev != NO_ENTRY && prev >= chunk_start && off <= 65535u) {
            if (src_u32(prev) == word) {
                let extra = count_match_extra(pos, prev, chunk_end);
                emit_seq(&op, lit_run, pos, off, extra);
                pos    += MIN_MATCH + extra;
                lit_run = pos;
                continue;
            }
        }

        pos += 1u;
    }

    // 残りリテラルをフラッシュ
    emit_final(&op, lit_run, chunk_end);
    dst_sizes[chunk_id] = op - op_base;
}
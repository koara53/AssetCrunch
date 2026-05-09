use std::fs;

pub struct MeshFile {
    pub data: Vec<u8>,
    pub ext: String,
    pub is_ascii_fbx: bool,
}

impl MeshFile {
    pub fn load(path: &str) -> Result<Self, String> {
        let data = fs::read(path).map_err(|e| e.to_string())?;
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin")
            .to_lowercase();

        let is_ascii_fbx = ext == "fbx"
            && data.len() > 4
            && &data[0..4] != b"Kayd"; // "Kaydara FBX Binary" = バイナリFBX

        Ok(Self { data, ext, is_ascii_fbx })
    }

    // .gcmesh フォーマット:
    //   magic(8) + ext_len(1) + ext + chunk_count(4)
    //   + chunk_sizes(4*n) + compressed_data
    pub fn write_gcmesh(
        path: &str,
        ext: &str,
        chunk_sizes: &[u32],
        packed: &[u8],
    ) {
        let mut out = Vec::new();
        out.extend_from_slice(b"GCMESH01");
        out.push(ext.len() as u8);
        out.extend_from_slice(ext.as_bytes());
        out.extend_from_slice(&(chunk_sizes.len() as u32).to_le_bytes());
        for &sz in chunk_sizes {
            out.extend_from_slice(&sz.to_le_bytes());
        }
        out.extend_from_slice(packed);
        fs::write(path, &out).expect("書き込み失敗");
    }


}
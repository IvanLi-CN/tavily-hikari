pub(crate) mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_web_assets.rs"));
}

pub fn embedded_bytes(path: &str) -> Option<&'static [u8]> {
    embedded::get(path)
}

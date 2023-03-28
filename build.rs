fn main() {
    #[cfg(windows)]
    embed_resource::compile("assets/steeve-sync.rc", embed_resource::NONE);
}

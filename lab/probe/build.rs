fn main() {
    embed_resource::compile("probe.rc", embed_resource::NONE)
        .manifest_required()
        .expect("the manifest must link, or Windows 8 refuses layered child windows");
}

use std::{path::Path};

fn main() -> color_eyre::Result<()> {
    blogamer::generate("output".into(), Path::new("example"))
}

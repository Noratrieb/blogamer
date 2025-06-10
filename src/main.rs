use clap::Parser;

fn main() -> color_eyre::Result<()> {
    let opts = blogamer::Opts::parse();
    blogamer::generate(opts)
}

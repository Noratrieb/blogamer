use color_eyre::{
    Result,
    eyre::{OptionExt, WrapErr, bail, ensure},
};
use pulldown_cmark::Options;
use std::{
    fs::DirEntry,
    path::{Path, PathBuf},
};

use crate::context::Context;

mod context {
    use std::collections::BTreeMap;

    #[derive(Default)]
    pub struct Context {
        output: OutputDirectory,
    }

    #[derive(Default)]
    struct OutputDirectory {
        entries: BTreeMap<String, OutputFile>
    }

    enum OutputFile {
        Dir(OutputDirectory),
        BinaryFile(Vec<u8>),
        StringFile(String),
    }

    impl Context {
        
    }
}

mod write {
    use color_eyre::{Result, eyre::Context};
    use std::path::Path;

    pub fn initialize(base: &Path) -> Result<()> {
        std::fs::remove_dir_all(base).wrap_err("deleting previous output")?;
        Ok(std::fs::create_dir_all(base).wrap_err("creating output")?)
    }

    pub fn write_file(base: &Path, name: &str, content: &str) -> Result<()> {
        Ok(std::fs::write(base.join(name), content)?)
    }
}

pub fn generate(out_base: PathBuf, root: &Path) -> Result<()> {
    let mut ctx = Context::default();

    collect_posts(&mut ctx, &root.join("posts"))
        .wrap_err_with(|| format!("reading posts from {}", root.display()))?;

    write::initialize(&out_base).wrap_err("initializing output")?;
    for output in ctx.outputs {
        match output {
            OutputFile::Post {
                name,
                frontmatter,
                html_body,
            } => {
                write::write_file(&out_base, &name, &html_body)?;
            }
        }
    }

    Ok(())
}

fn collect_posts(ctx: &mut Context, path: &Path) -> Result<()> {
    let entries = std::fs::read_dir(path)?;

    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_eyre("invalid UTF-8 filename")?;

        collect_post(ctx, &entry, name).wrap_err_with(|| format!("generating post {name}"))?;
    }

    Ok(())
}

fn collect_post(ctx: &mut Context, entry: &DirEntry, name: &str) -> Result<()> {
    let meta = entry.metadata()?;
    if meta.is_dir() {
        todo!("directory post");
    }

    let Some((name, ext)) = name.split_once('.') else {
        bail!("invalid post filename {name}, must be *.md");
    };
    ensure!(
        ext == "md",
        "invalid filename {name}, only .md extensions are allowed"
    );

    let content = std::fs::read_to_string(entry.path()).wrap_err("reading contents")?;

    let rest = content
        .strip_prefix("---\n")
        .ok_or_eyre("post must start with `---`")?;
    let (frontmatter, body) = rest
        .split_once("---\n")
        .ok_or_eyre("unterminated frontmatter, needs another `---`")?;

    let frontmatter =
        serde_norway::from_str::<Frontmatter>(frontmatter).wrap_err("¡nvalid frontmatter")?;

    let html_body = parse_post_body(&body).wrap_err("parsing post")?;

    ctx.outputs.push(OutputFile::Post {
        name: name.to_owned(),
        frontmatter,
        html_body,
    });

    Ok(())
}

#[derive(serde::Deserialize)]
struct Frontmatter {
    title: String,
    date: String,
}

fn parse_post_body(content: &str) -> Result<String> {
    let mut options = pulldown_cmark::Options::empty();
    options |= Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH;
    let parser = pulldown_cmark::Parser::new_ext(content, options);

    let mut output = String::new();
    pulldown_cmark::html::push_html(&mut output, parser);
    Ok(output)
}

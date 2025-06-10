use askama::Template;
use color_eyre::{
    Result,
    eyre::{OptionExt, WrapErr, bail, ensure},
};
use pulldown_cmark::{Event, Options, Tag, TagEnd};
use sha2::Digest;
use std::{
    collections::HashMap,
    fs::DirEntry,
    io,
    path::{Path, PathBuf},
};

#[derive(clap::Parser)]
pub struct Opts {
    #[clap(long)]
    optimize: bool,
    #[clap(long, short)]
    input: PathBuf,
    #[clap(long, short)]
    output: PathBuf,
}

pub struct Context {
    opts: Opts,
    static_files: HashMap<String, Vec<u8>>,
    theme_css_path: String,
}

struct PictureImages {
    sources: Vec<PictureSource>,
    fallback_path: String,
    height: u32,
    width: u32,
}

struct PictureSource {
    path: String,
    media_type: String,
}

impl Context {
    fn add_static_file(&mut self, name: &str, ext: &str, content: Vec<u8>) -> Result<String> {
        let name = format!("{name}-{}{ext}", create_hash_string(&content));
        let _ = self.static_files.insert(name.clone(), content);
        Ok(format!("/static/{name}"))
    }

    fn add_image(&mut self, path: &Path) -> Result<PictureImages> {
        let image = image::ImageReader::open(path)
            .wrap_err("reading image")?
            .decode()
            .wrap_err("decoding image")?;

        let name = path
            .file_stem()
            .ok_or_eyre("image does not have name")?
            .to_str()
            .unwrap();

        let optimize = self.opts.optimize;

        let mut encode = |format, ext| -> Result<_> {
            let mut bytes = vec![];
            image.write_to(&mut io::Cursor::new(&mut bytes), format)?;

            self.add_static_file(name, ext, bytes)
        };

        let fallback_path = encode(image::ImageFormat::Jpeg, ".jpg")?;

        let sources = if optimize {
            let avif_path = encode(image::ImageFormat::Avif, ".avif")?;
            let webp_path = encode(image::ImageFormat::WebP, ".webp")?;
            vec![
                PictureSource {
                    path: avif_path,
                    media_type: "image/avif".to_owned(),
                },
                PictureSource {
                    path: webp_path,
                    media_type: "image/webp".to_owned(),
                },
            ]
        } else {
            vec![]
        };

        Ok(PictureImages {
            sources,
            fallback_path,
            height: image.height(),
            width: image.width(),
        })
    }
}

fn create_hash_string(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    bs58::encode(&digest[..16]).into_string()
}

struct Post {
    name: String,
    relative_to: PathBuf,
    frontmatter: Frontmatter,
    body_md: String,
}

mod write {
    use color_eyre::{Result, eyre::Context};
    use std::path::Path;

    pub fn initialize(base: &Path) -> Result<()> {
        let _ = std::fs::remove_dir_all(base).wrap_err("deleting previous output");
        Ok(std::fs::create_dir_all(base).wrap_err("creating output")?)
    }
}

pub fn generate(opts: Opts) -> Result<()> {
    let mut ctx = Context {
        opts,
        static_files: Default::default(),
        theme_css_path: String::new(),
    };

    ctx.theme_css_path = ctx
        .add_static_file(
            "theme",
            ".css",
            include_bytes!("../templates/theme.css")
                .as_slice()
                .to_owned(),
        )
        .wrap_err("adding theme.css")?;

    let posts = collect_posts(&ctx.opts.input.join("posts"))
        .wrap_err_with(|| format!("reading posts from {}", ctx.opts.input.display()))?;

    write::initialize(&ctx.opts.output).wrap_err("initializing output")?;

    for post in posts {
        let dir = ctx.opts.output.join("blog").join("posts").join(&post.name);
        std::fs::create_dir_all(&dir)?;

        let html = render_post(&mut ctx, &post)?;

        std::fs::write(dir.join("index.html"), html)?;
    }

    let static_dir = ctx.opts.output.join("static");
    std::fs::create_dir(&static_dir).wrap_err("creating static")?;
    for (name, content) in ctx.static_files {
        std::fs::write(static_dir.join(name), content).wrap_err("writing static file")?;
    }

    Ok(())
}

fn collect_posts(path: &Path) -> Result<Vec<Post>> {
    let mut posts = vec![];
    let entries = std::fs::read_dir(path)?;

    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_eyre("invalid UTF-8 filename")?;

        let post =
            collect_post(&entry, name).wrap_err_with(|| format!("generating post {name}"))?;
        posts.push(post);
    }

    Ok(posts)
}

fn collect_post(entry: &DirEntry, name: &str) -> Result<Post> {
    let meta = entry.metadata()?;

    let (name, content, relative_to) = if meta.is_dir() {
        let content_path = entry.path().join("index.md");
        let content = std::fs::read_to_string(&content_path)
            .wrap_err_with(|| format!("could not read {}", content_path.display()))?;

        (name.to_owned(), content, entry.path())
    } else {
        let Some((name, ext)) = name.split_once('.') else {
            bail!("invalid post filename {name}, must be *.md");
        };
        ensure!(
            ext == "md",
            "invalid filename {name}, only .md extensions are allowed"
        );
        let content = std::fs::read_to_string(entry.path()).wrap_err("reading contents")?;

        (
            name.to_owned(),
            content,
            entry.path().parent().unwrap().to_owned(),
        )
    };

    let rest = content
        .strip_prefix("---\n")
        .ok_or_eyre("post must start with `---`")?;
    let (frontmatter, body) = rest
        .split_once("---\n")
        .ok_or_eyre("unterminated frontmatter, needs another `---`")?;

    let frontmatter =
        serde_norway::from_str::<Frontmatter>(frontmatter).wrap_err("¡nvalid frontmatter")?;

    Ok(Post {
        name,
        frontmatter,
        body_md: body.to_owned(),
        relative_to,
    })
}

#[derive(serde::Deserialize)]
struct Frontmatter {
    title: String,
    date: String,
}

fn render_post(ctx: &mut Context, post: &Post) -> Result<String> {
    #[derive(askama::Template)]
    #[template(path = "../templates/post.html")]
    struct PostTemplate<'a> {
        title: &'a str,
        body: &'a str,
        theme_css_path: &'a str,
    }

    let body = render_body(ctx, &post.relative_to, &post.body_md)?;

    PostTemplate {
        title: &post.frontmatter.title,
        body: &body,
        theme_css_path: &ctx.theme_css_path,
    }
    .render()
    .wrap_err("failed to render template")
}

fn render_body(ctx: &mut Context, relative_to: &Path, md: &str) -> Result<String> {
    let mut options = pulldown_cmark::Options::empty();
    options |= Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH;
    let mut parser = pulldown_cmark::Parser::new_ext(md, options);

    let mut events = vec![];

    while let Some(ev) = parser.next() {
        dbg!(&ev);
        match ev {
            Event::Start(Tag::Image {
                link_type: _,
                dest_url,
                title: _,
                id: _,
            }) => {
                let Some(Event::Text(alt)) = parser.next() else {
                    bail!("No alt text for image tag");
                };
                let Some(Event::End(TagEnd::Image)) = parser.next() else {
                    bail!("No end tag for image");
                };

                let sources = ctx.add_image(&relative_to.join(dest_url.as_ref()))?;

                events.extend([
                    Event::Start(Tag::HtmlBlock),
                    Event::Html("<picture>".into()),
                ]);
                for source in sources.sources {
                    events.push(Event::Html(
                        format!(
                            r#"<source srcset="{}" type="{}">"#,
                            source.path, source.media_type
                        )
                        .into(),
                    ));
                }
                events.extend([
                    Event::Html(
                        format!(
                            r#"<img src="{}" alt="{}" height="{}" width="{}">"#,
                            sources.fallback_path, alt, sources.height, sources.width
                        )
                        .into(),
                    ),
                    Event::Html("</picture>".into()),
                    Event::End(TagEnd::HtmlBlock),
                ]);
            }
            ev => events.push(ev),
        }
    }

    let mut body = String::new();
    pulldown_cmark::html::push_html(&mut body, events.into_iter());
    Ok(body)
}

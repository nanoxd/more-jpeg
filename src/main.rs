use http_types::Mime;
use image::{imageops::FilterType, jpeg::JPEGEncoder, DynamicImage, GenericImageView};
use liquid::{Object, Template};
use rand::Rng;
use serde::Serialize;
use std::{collections::HashMap, error::Error, net::SocketAddr, sync::Arc};
use tokio::{fs::read_to_string, sync::RwLock};
use ulid::Ulid;
use warp::Filter;

mod mimes;

pub type TemplateMap = HashMap<String, Template>;
pub const JPEG_QUALITY: u8 = 25;

struct State {
    templates: TemplateMap,
    images: RwLock<HashMap<Ulid, Image>>,
}

impl State {
    fn new(templates: TemplateMap) -> Self {
        State {
            templates,
            images: Default::default(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum TemplateError {
    #[error("invalid template path: {0}")]
    InvalidTemplatePath(String),

    #[error("template not found: {0}")]
    TemplateNotFound(String),

    #[error("Invalid ID for image")]
    InvalidID,
}

#[derive(Serialize)]
struct UploadResponse<'a> {
    src: &'a str,
}

struct Image {
    mime: Mime,
    contents: Vec<u8>,
}

trait MimeAware {
    fn content_type(self, mime: Mime) -> Self;
}

impl MimeAware for http::response::Builder {
    fn content_type(self, mime: Mime) -> Self {
        self.header("content-type", mime.to_string())
    }
}

trait ForWarp {
    type Reply;

    fn for_warp(self) -> Result<Self::Reply, warp::Rejection>;
}

impl<T> ForWarp for Result<T, Box<dyn Error>>
where
    T: warp::Reply + 'static,
{
    type Reply = Box<dyn warp::Reply>;

    fn for_warp(self) -> Result<Self::Reply, warp::Rejection> {
        let b: Box<dyn warp::Reply> = match self {
            Ok(reply) => Box::new(reply),
            Err(e) => {
                log::error!("Error: {}", e);
                let res = http::Response::builder()
                    .status(500)
                    .body("Something went wrong, sorry");
                Box::new(res)
            }
        };
        Ok(b)
    }
}

trait BitCrush: Sized {
    type Error;

    fn bitcrush(self) -> Result<Self, Self::Error>;
}

impl BitCrush for DynamicImage {
    type Error = image::ImageError;

    fn bitcrush(self) -> Result<Self, Self::Error> {
        let mut current = self;
        let (orig_w, orig_h) = current.dimensions();

        let mut rng = rand::thread_rng();
        let (temp_w, temp_h) = (
            rng.gen_range(orig_w / 2, orig_w * 2),
            rng.gen_range(orig_h / 2, orig_h * 2),
        );

        let mut out: Vec<u8> = Default::default();
        for _ in 0..2 {
            current = current
                .resize_exact(temp_w, temp_h, FilterType::Nearest)
                .rotate180()
                .huerotate(180);
            out.clear();
            {
                let mut encoder = JPEGEncoder::new_with_quality(&mut out, rng.gen_range(10, 30));
                encoder.encode_image(&current)?;
            }

            current = image::load_from_memory_with_format(&out[..], image::ImageFormat::Jpeg)?
                .resize_exact(orig_w, orig_h, FilterType::Nearest);
        }

        Ok(current)
    }
}

async fn compile_templates(paths: &[&str]) -> Result<TemplateMap, Box<dyn Error>> {
    let compiler = liquid::ParserBuilder::with_stdlib().build()?;

    let mut map = TemplateMap::new();
    for path in paths {
        let name = path
            .split("/")
            .last()
            .map(|name| name.trim_end_matches(".liquid"))
            .ok_or_else(|| TemplateError::InvalidTemplatePath(path.to_string()))?;
        let source = read_to_string(path).await?;
        let template = compiler.parse(&source)?;
        map.insert(name.to_string(), template);
    }

    Ok(map)
}

async fn serve_template(
    state: &State,
    name: &str,
    mime: Mime,
) -> Result<impl warp::Reply, Box<dyn Error>> {
    let template = state
        .templates
        .get(name)
        .ok_or_else(|| TemplateError::TemplateNotFound(name.to_string()))?;

    let globals: Object = Default::default();
    let markup = template.render(&globals)?;

    Ok(http::Response::builder().content_type(mime).body(markup))
    // let mut res = Response::new(StatusCode::Ok);
    // res.set_content_type(mime);
    // res.set_body(markup);
    // Ok(res)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::init();

    let templates = compile_templates(&[
        "./templates/index.html.liquid",
        "./templates/style.css.liquid",
        "./templates/main.js.liquid",
    ])
    .await?;

    log::info!("{} templates compiled", templates.len());

    let state = State::new(templates);
    let state = Arc::new(state);

    let with_state = {
        let filter = warp::filters::any::any().map(move || state.clone());
        move || filter.clone()
    };

    let index = warp::filters::method::get()
        .and(warp::path::end())
        .and(with_state())
        .and_then(|state: Arc<State>| async move {
            serve_template(&state, "index.html", mimes::html())
                .await
                .for_warp()
        });

    let style = warp::filters::method::get()
        .and(warp::path!("style.css"))
        .and(with_state())
        .and_then(|state: Arc<State>| async move {
            serve_template(&state, "style.css", mimes::css())
                .await
                .for_warp()
        });

    let js = warp::filters::method::get()
        .and(warp::path!("main.js"))
        .and(with_state())
        .and_then(|state: Arc<State>| async move {
            serve_template(&state, "main.js", mimes::js())
                .await
                .for_warp()
        });

    let upload = warp::filters::method::post()
        .and(warp::path("upload"))
        .and(with_state())
        .and(warp::filters::body::bytes())
        .and_then(|state: Arc<State>, bytes: bytes::Bytes| async move {
            handle_upload(&state, bytes).await.for_warp()
        });

    let images = warp::filters::method::get()
        .and(warp::path!("images" / String))
        .and(with_state())
        .and_then(|name: String, state: Arc<State>| async move {
            serve_image(&state, &name).await.for_warp()
        });

    let addr: SocketAddr = "127.0.0.1:3000".parse()?;
    warp::serve(index.or(style).or(js).or(upload).or(images))
        .run(addr)
        .await;
    Ok(())
}

async fn serve_image(state: &State, name: &str) -> Result<impl warp::Reply, Box<dyn Error>> {
    let id: Ulid = name
        .trim_end_matches(".jpg")
        .parse()
        .map_err(|_| TemplateError::InvalidID)?;

    let images = state.images.read().await;
    let res: Box<dyn warp::Reply> = if let Some(img) = images.get(&id) {
        Box::new(
            http::Response::builder()
                .content_type(img.mime.clone())
                .body(img.contents.clone()),
        )
    } else {
        Box::new(
            http::Response::builder()
                .status(404)
                .body("Image not found"),
        )
    };

    Ok(res)
}

async fn handle_upload(
    state: &State,
    bytes: bytes::Bytes,
) -> Result<impl warp::Reply, Box<dyn Error>> {
    let img = image::load_from_memory(&bytes[..])?.bitcrush()?;
    let mut output: Vec<u8> = Default::default();
    let mut encoder = JPEGEncoder::new_with_quality(&mut output, JPEG_QUALITY);
    encoder.encode_image(&img)?;

    let id = Ulid::new();
    let src = format!("/images/{}", id);

    let img = Image {
        mime: mimes::jpeg(),
        contents: output,
    };

    {
        let mut images = state.images.write().await;
        images.insert(id, img);
    }

    let payload = serde_json::to_string(&UploadResponse { src: &src })?;
    let res = http::Response::builder()
        .content_type(mimes::json())
        .body(payload);
    Ok(res)
}

use async_std::fs::read_to_string;
use liquid::{Object, Template};
use serde::Serialize;
use std::{collections::HashMap, error::Error};
use tide::{http::Mime, Request, Response, StatusCode};

mod mimes;

pub type TemplateMap = HashMap<String, Template>;
struct State {
    templates: TemplateMap,
}

impl State {
    fn new(templates: TemplateMap) -> Self {
        State { templates }
    }
}

trait ForTide {
    fn for_tide(self) -> Result<tide::Response, tide::Error>;
}

impl ForTide for Result<tide::Response, Box<dyn Error>> {
    fn for_tide(self) -> Result<tide::Response, tide::Error> {
        self.map_err(|e| {
            log::error!("While serving template: {}", e);
            tide::Error::from_str(
                StatusCode::InternalServerError,
                "Something went wrong, sorry!",
            )
        })
    }
}

#[derive(Debug, thiserror::Error)]
enum TemplateError {
    #[error("invalid template path: {0}")]
    InvalidTemplatePath(String),

    #[error("template not found: {0}")]
    TemplateNotFound(String),
}

#[derive(Serialize)]
struct UploadResponse<'a> {
    src: &'a str,
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
    templates: &TemplateMap,
    name: &str,
    mime: Mime,
) -> Result<Response, Box<dyn Error>> {
    let template = templates
        .get(name)
        .ok_or_else(|| TemplateError::TemplateNotFound(name.to_string()))?;
    let globals: Object = Default::default();
    let markup = template.render(&globals).unwrap();
    let mut res = Response::new(StatusCode::Ok);
    res.set_content_type(mime);
    res.set_body(markup);
    Ok(res)
}

#[async_std::main]
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

    let mut app = tide::with_state(State::new(templates));

    app.at("/").get(|req: Request<State>| async move {
        log::info!("Serving /");
        let name = "index.html";
        serve_template(&req.state().templates, name, mimes::html())
            .await
            .for_tide()
    });

    app.at("/style.css").get(|req: Request<State>| async move {
        serve_template(&req.state().templates, "style.css", mimes::css())
            .await
            .for_tide()
    });

    app.at("/main.js").get(|req: Request<State>| async move {
        serve_template(&req.state().templates, "main.js", mimes::js())
            .await
            .for_tide()
    });

    app.at("/upload")
        .post(|mut req: Request<State>| async move {
            let body = req.body_bytes().await?;
            let img = image::load_from_memory(&body[..])?;
            let mut output: Vec<u8> = Default::default();
            let mut encoder = image::jpeg::JPEGEncoder::new_with_quality(&mut output, 90);
            encoder.encode_image(&img)?;

            let payload = base64::encode(output);
            let src = format!("data:image/jpeg;base64,{}", payload);

            let mut res = Response::new(StatusCode::Ok);
            res.set_content_type(mimes::json());
            res.set_body(tide::Body::from_json(&UploadResponse { src: &src })?);
            Ok(res)
        });

    app.listen("localhost:3000").await?;
    Ok(())
}

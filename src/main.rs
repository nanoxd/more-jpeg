use async_std::fs::read_to_string;
use liquid::Object;
use std::{error::Error, str::FromStr};
use tide::{http::Mime, Response, StatusCode};

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::init();

    let mut app = tide::new();

    app.at("/").get(|_| async {
        log::info!("Serving /");
        let path = "./templates/index.html.liquid";
        serve_template(path).await.map_err(|e| {
            log::error!("While serving template: {}", e);
            tide::Error::from_str(
                StatusCode::InternalServerError,
                "Something went wrong, sorry!",
            )
        })
    });

    app.listen("localhost:3000").await?;
    Ok(())
}

async fn serve_template(path: &str) -> Result<Response, Box<dyn Error>> {
    let source = read_to_string(path).await.unwrap();
    let compiler = liquid::ParserBuilder::with_stdlib().build().unwrap();
    let template = compiler.parse(&source).unwrap();
    let globals: Object = Default::default();
    let markup = template.render(&globals).unwrap();
    let mut res = Response::new(StatusCode::Ok);
    res.set_content_type(Mime::from_str("text/html; charset=utf-8").unwrap());
    res.set_body(markup);
    Ok(res)
}

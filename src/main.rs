use async_std::fs::read_to_string;
use liquid::Object;
use std::{error::Error, str::FromStr};
use tide::{http::Mime, Response, StatusCode};

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut app = tide::new();

    app.at("/").get(|_| async {
        let path = "./templates/index.html.liquid";
        let source = read_to_string(path).await.unwrap();
        let compiler = liquid::ParserBuilder::with_stdlib().build().unwrap();
        let template = compiler.parse(&source).unwrap();
        let globals: Object = Default::default();
        let markup = template.render(&globals).unwrap();
        let mut res = Response::new(StatusCode::Ok);
        res.set_content_type(Mime::from_str("text/html; charset=utf-8").unwrap());
        res.set_body(markup);
        Ok(res)
    });

    app.listen("localhost:3000").await?;
    Ok(())
}

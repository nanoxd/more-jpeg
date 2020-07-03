use std::error::Error;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut app = tide::new();

    app.at("/").get(|_| async { Ok("Hello from Tide") });

    app.listen("localhost:3000").await?;
    Ok(())
}

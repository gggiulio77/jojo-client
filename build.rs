// Necessary because of this issue: https://github.com/rust-lang/cargo/issues/9641
fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();

    for (key, value) in std::env::vars() {
        println!("cargo:rustc-env={key}={value}");
    }

    embuild::build::CfgArgs::output_propagated("ESP_IDF")?;
    embuild::build::LinkArgs::output_propagated("ESP_IDF")?;
    Ok(())
}

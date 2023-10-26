fn main() {
    dotenv::dotenv().ok();

    for (key, value) in std::env::vars() {
        println!("cargo:rustc-env={key}={value}");
    }

    embuild::espidf::sysenv::output();
}

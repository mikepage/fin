fn main() {
    match fin_lib::cli::run(std::env::args().skip(1).collect()) {
        Ok(serde_json::Value::String(text)) => print!("{text}"),
        Ok(out) => println!("{}", serde_json::to_string_pretty(&out).expect("JSON")),
        Err(e) => {
            eprintln!("{}", serde_json::json!({ "error": e }));
            std::process::exit(1);
        }
    }
}

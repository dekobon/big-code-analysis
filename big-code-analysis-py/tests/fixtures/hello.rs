fn hello(name: &str) -> String {
    if name.is_empty() {
        return String::from("hello, world");
    }
    format!("hello, {name}")
}

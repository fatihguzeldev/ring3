#[path = "../tests/support/fixture.rs"]
mod fixture;

fn main() {
    println!("{}", serde_json::json!(fixture::executable()));
}

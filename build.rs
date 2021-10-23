use openapiv3::OpenAPI;
use serde_yaml::{from_str, Error};

fn main() {
    let text = include_str!("res/api.yaml");
    let result: Result<OpenAPI, Error> = from_str(text);
    if let Err(e) = result {
        panic!("{}", e);
    }
}

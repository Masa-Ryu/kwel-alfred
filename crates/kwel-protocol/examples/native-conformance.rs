//! Internal cross-language test adapter; never packaged as a product CLI.
use kwel_protocol::{MAX_MESSAGE_BYTES, decode, encode_frame};
use std::io::{self, BufRead};

fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.expect("test input");
        let result =
            decode(line.as_bytes()).and_then(|message| encode_frame(&message, MAX_MESSAGE_BYTES));
        let value = match result {
            Ok(frame) => {
                serde_json::json!({"accepted":true,"message":serde_json::from_slice::<serde_json::Value>(&frame[4..]).unwrap()})
            }
            Err(_) => serde_json::json!({"accepted":false}),
        };
        println!("{value}");
    }
}

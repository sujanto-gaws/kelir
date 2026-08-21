//! The Tamper-Proof Pattern (Registry S7.1) driven end to end against
//! `jsonlogic-rs`, with a tampered `grand_total` in the payload.
use serde_json::json;

fn main() {
    let calculate = json!({"sum": [{"map": [{"var": "items"},
        {"*": [{"var": "unit_price"}, {"var": "quantity"}]}]}]});

    let mut payload = json!({
        "items": [{"unit_price": 10, "quantity": 2}, {"unit_price": 5.5, "quantity": 4}],
        "grand_total": 0.01
    });

    match jsonlogic_rs::apply(&calculate, &payload) {
        Ok(recalculated) => {
            println!("recalculated  = {recalculated}");
            println!("was an error  = false  (there is nothing to reject on)");
            payload["grand_total"] = recalculated.clone();
            // The registry's S7.3 wrapper, as a backend would apply it.
            let normalized = recalculated.as_f64().unwrap_or(0.0);
            println!("after wrapper = {normalized}");
            println!("persisted     = {}", json!({"grand_total": normalized}));
            println!("true total    = 42");
        }
        Err(error) => println!("rejected: {error}"),
    }
}

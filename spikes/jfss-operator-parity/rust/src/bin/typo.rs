use serde_json::json;
fn main() {
    for expr in [json!({"mapp": [[1, 2], {"var": ""}]}), json!({"sum": [[1, 2]]})] {
        println!("{expr}  ->  {:?}", jsonlogic_rs::apply(&expr, &json!({})));
    }
}

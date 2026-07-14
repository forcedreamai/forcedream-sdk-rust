use forcedream::ForceDream;

fn main() {
    println!("=== Real signup ===");
    let email = format!("rust-sdk-test-{}@example.com", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
    let signup = ForceDream::signup(&email).expect("signup should succeed");
    println!("Signed up: user_id={}, trial_balance={}", signup.user_id, signup.trial_balance_gbp);

    let client = ForceDream::new(Some(signup.live_key.clone()));

    println!();
    println!("=== search_agents (client-side filtered) ===");
    let search = client.search_agents(Some("data:extraction"), None).expect("search should succeed");
    println!("{}", serde_json::to_string_pretty(&search).unwrap());

    println!();
    println!("=== invoke (real agent, real charge) ===");
    let result = client
        .invoke("data-extract-v1", "Extract the year and location from: Founded in 2003 in Berlin, Germany.", Some(60))
        .expect("invoke should not error");
    println!("{:#?}", result);

    if result.status == "completed" {
        if let Some(task_id) = &result.task_id {
            println!();
            println!("=== verify (real Ed25519 proof) ===");
            let verify_result = client.verify_by_task_id(task_id).expect("verify should not error");
            println!("{:#?}", verify_result);
        }
    }
}

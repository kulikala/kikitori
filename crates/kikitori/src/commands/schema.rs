pub fn run(name: &str) -> Result<(), String> {
    match name {
        "transcript" | "transcript-v1" | "bundle" | "doctor" | "engines" => {}
        _ => {
            return Err(format!(
            "スキーマ名 `{name}` が不明です。transcript、transcript-v1、bundle、doctor、engines から選んでください。"
        ))
        }
    }
    let data = match name {
        "transcript" => include_str!("../../schemas/transcript.json"),
        "transcript-v1" => include_str!("../../schemas/transcript-v1.json"),
        "bundle" => include_str!("../../schemas/bundle.json"),
        "doctor" => include_str!("../../schemas/doctor.json"),
        "engines" => include_str!("../../schemas/engines.json"),
        _ => unreachable!(),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::from_str::<serde_json::Value>(data).map_err(|e| e.to_string())?
        )
        .map_err(|e| e.to_string())?
    );
    Ok(())
}

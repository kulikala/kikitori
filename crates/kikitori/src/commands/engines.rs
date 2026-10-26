use crate::EngineAction;

pub fn run(_action: EngineAction) -> Result<(), String> {
    let mut rows = vec![
        serde_json::json!({"name":"reazon","role":"asr","models":["ja"],"dtypes":["fp32"],"timestamps":false,"source":"GitHub sherpa-onnx release","backend":"onnxruntime"}),
        serde_json::json!({"name":"reazon-greedy","role":"asr","models":["ja","ja-en"],"dtypes":["fp32","fp16","int8"],"timestamps":false,"source":"GitHub sherpa-onnx release","backend":"sherpa-onnx"}),
        serde_json::json!({"name":"silero-vad","role":"vad","model":"silero_vad.onnx","source":kikitori_fetch::CORE_ASSETS.iter().find(|a|a.name=="vad").unwrap().source}),
        serde_json::json!({"name":"pyannote","role":"diarizer","model":"segmentation/model.onnx","source":kikitori_fetch::CORE_ASSETS.iter().find(|a|a.name=="segmentation").unwrap().source}),
        serde_json::json!({"name":"campplus","role":"diarizer","model":"embedding","source":kikitori_fetch::CORE_ASSETS.iter().find(|a|a.name=="embedding").unwrap().source}),
    ];
    for model in kikitori_fetch::MODELS {
        rows.push(serde_json::json!({"name":model.name,"role":model.role,"source":model.source,"license":model.license}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?
    );
    Ok(())
}

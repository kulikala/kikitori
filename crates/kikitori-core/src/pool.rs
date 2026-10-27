//! 同じ認識器を複数読み込み、区間を並列に起こす。区間ごとの結果は、1つで順に起こしたときと同じ。

use crate::{AsrEngine, AsrOutput, AsrRequest, Capabilities};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

pub struct AsrPool {
    engines: Vec<Box<dyn AsrEngine>>,
}

impl AsrPool {
    /// `engines` は同じ設定で作った認識器。1つ目の id と capabilities を代表に使う。
    pub fn new(engines: Vec<Box<dyn AsrEngine>>) -> Self {
        assert!(!engines.is_empty(), "AsrPool needs at least one engine");
        Self { engines }
    }
}

impl AsrEngine for AsrPool {
    fn id(&self) -> &str {
        self.engines[0].id()
    }

    fn capabilities(&self) -> &Capabilities {
        self.engines[0].capabilities()
    }

    fn transcribe(&mut self, requests: &[AsrRequest<'_>]) -> Result<Vec<AsrOutput>, String> {
        if self.engines.len() == 1 || requests.len() < 2 {
            return self.engines[0].transcribe(requests);
        }
        // 区間を1つずつ、空いた認識器が取る。長さの違う区間が混ざっても、手の空く認識器が出ない
        let next = AtomicUsize::new(0);
        let slots: Vec<Mutex<Option<Result<AsrOutput, String>>>> =
            requests.iter().map(|_| Mutex::new(None)).collect();
        std::thread::scope(|scope| {
            for engine in &mut self.engines {
                let (next, slots) = (&next, &slots);
                scope.spawn(move || loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= requests.len() {
                        break;
                    }
                    let result =
                        engine
                            .transcribe(&requests[index..=index])
                            .and_then(|mut outputs| {
                                if outputs.len() == 1 {
                                    Ok(outputs.remove(0))
                                } else {
                                    Err(format!(
                                        "ASR returned {} outputs for 1 span",
                                        outputs.len()
                                    ))
                                }
                            });
                    let failed = result.is_err();
                    *slots[index].lock().unwrap() = Some(result);
                    if failed {
                        // 残りを取らせない
                        next.store(requests.len(), Ordering::Relaxed);
                        break;
                    }
                });
            }
        });
        let mut outputs = Vec::with_capacity(requests.len());
        for slot in slots {
            match slot.into_inner().unwrap() {
                Some(Ok(output)) => outputs.push(output),
                Some(Err(error)) => return Err(error),
                None => {}
            }
        }
        if outputs.len() != requests.len() {
            return Err("ASR stopped before all spans were transcribed".to_owned());
        }
        Ok(outputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Token;

    /// 波形の長さと、どの認識器が起こしたかを返す
    struct Fake {
        name: String,
        capabilities: Capabilities,
        fail_on: Option<usize>,
    }

    impl AsrEngine for Fake {
        fn id(&self) -> &str {
            &self.name
        }
        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }
        fn transcribe(&mut self, requests: &[AsrRequest<'_>]) -> Result<Vec<AsrOutput>, String> {
            requests
                .iter()
                .map(|request| {
                    if Some(request.pcm.len()) == self.fail_on {
                        return Err("boom".to_owned());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(request.pcm.len() as u64));
                    Ok(AsrOutput {
                        text: request.pcm.len().to_string(),
                        tokens: Vec::<Token>::new(),
                    })
                })
                .collect()
        }
    }

    fn pool(n: usize, fail_on: Option<usize>) -> AsrPool {
        AsrPool::new(
            (0..n)
                .map(|i| {
                    Box::new(Fake {
                        name: format!("fake{i}"),
                        capabilities: Capabilities {
                            timestamps: false,
                            context: false,
                            max_seconds: 20.0,
                            dtypes: &[],
                        },
                        fail_on,
                    }) as Box<dyn AsrEngine>
                })
                .collect(),
        )
    }

    fn requests(waves: &[Vec<f32>]) -> Vec<AsrRequest<'_>> {
        waves
            .iter()
            .map(|pcm| AsrRequest {
                pcm,
                context: &[],
                language: None,
                want_timestamps: false,
            })
            .collect()
    }

    #[test]
    fn outputs_keep_the_order_of_the_requests() {
        let waves: Vec<Vec<f32>> = [9, 1, 5, 2, 8, 3, 7]
            .iter()
            .map(|&n| vec![0.0; n])
            .collect();
        let texts: Vec<String> = pool(3, None)
            .transcribe(&requests(&waves))
            .unwrap()
            .into_iter()
            .map(|o| o.text)
            .collect();
        assert_eq!(texts, ["9", "1", "5", "2", "8", "3", "7"]);
        assert_eq!(pool(3, None).id(), "fake0");
    }

    #[test]
    fn an_error_from_any_engine_is_returned() {
        let waves: Vec<Vec<f32>> = [1, 2, 3, 4].iter().map(|&n| vec![0.0; n]).collect();
        assert_eq!(
            pool(2, Some(3)).transcribe(&requests(&waves)).unwrap_err(),
            "boom"
        );
    }
}

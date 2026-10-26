//! Minimal kaldi-native-fbank binding used by the ReazonSpeech recognizer.
//!
//! The C++ sources are compiled with `-Dknf=knf_rs`, so these symbols cannot
//! collide with the copy of kaldi-native-fbank that ships inside sherpa-onnx.

#[repr(C)]
pub struct FbankResult {
    pub frames: *mut f32,
    pub num_frames: i32,
    pub num_bins: i32,
}

extern "C" {
    pub fn ComputeFbank(waveform: *const f32, waveform_size: i32) -> FbankResult;
    pub fn DestroyFbankResult(result: *mut FbankResult);
}

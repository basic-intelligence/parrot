use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecordedAudio {
    pub samples: Vec<f32>,
    pub sample_rate_hz: u32,
    pub channels: u16,
}

pub fn trim_for_dictation(samples: &[f32]) -> Vec<f32> {
    trim_for_dictation_with_options(samples, 16_000, 20, 600, 80)
}

pub fn trim_recorded_audio_for_dictation(audio: &RecordedAudio) -> RecordedAudio {
    RecordedAudio {
        samples: trim_for_dictation_with_options(&audio.samples, audio.sample_rate_hz, 20, 600, 80),
        sample_rate_hz: audio.sample_rate_hz,
        channels: audio.channels,
    }
}

pub fn trim_for_dictation_with_options(
    samples: &[f32],
    sample_rate_hz: u32,
    frame_milliseconds: u32,
    padding_milliseconds: u32,
    minimum_speech_milliseconds: u32,
) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }

    let frame_size = ((sample_rate_hz * frame_milliseconds) / 1000).max(1) as usize;
    let frame_count = samples.len() / frame_size;
    if frame_count == 0 {
        return samples.to_vec();
    }

    let mut rms_values = Vec::with_capacity(frame_count);
    for frame in 0..frame_count {
        let start = frame * frame_size;
        let end = samples.len().min(start + frame_size);
        let sum = samples[start..end]
            .iter()
            .map(|sample| sample * sample)
            .sum::<f32>();
        rms_values.push((sum / (end - start).max(1) as f32).sqrt());
    }

    let mut sorted = rms_values.clone();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let noise_floor_index = (sorted.len() / 10).min(sorted.len() - 1);
    let noise_floor = sorted[noise_floor_index];
    let threshold = 0.008_f32.max(noise_floor * 3.0);

    let Some(first_speech) = rms_values.iter().position(|value| *value >= threshold) else {
        return Vec::new();
    };
    let Some(last_speech) = rms_values.iter().rposition(|value| *value >= threshold) else {
        return Vec::new();
    };

    let speech_frames = last_speech - first_speech + 1;
    let minimum_speech_frames = (minimum_speech_milliseconds / frame_milliseconds).max(1) as usize;
    if speech_frames < minimum_speech_frames {
        return Vec::new();
    }

    let padding_frames = (padding_milliseconds / frame_milliseconds).max(1) as usize;
    let first_frame = first_speech.saturating_sub(padding_frames);
    let last_frame = (last_speech + padding_frames).min(frame_count - 1);
    let start_sample = first_frame * frame_size;
    let end_sample = samples.len().min((last_frame + 1) * frame_size);

    samples[start_sample..end_sample].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Fixture {
        name: String,
        samples: Vec<f32>,
        sample_rate_hz: u32,
        frame_milliseconds: u32,
        padding_milliseconds: u32,
        minimum_speech_milliseconds: u32,
        expected: Vec<f32>,
    }

    #[test]
    fn matches_shared_trimmer_fixtures() {
        let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
            "../../../native-core/shared/test-fixtures/speech-activity-trimmer.json"
        ))
        .unwrap();

        for fixture in fixtures {
            assert_eq!(
                trim_for_dictation_with_options(
                    &fixture.samples,
                    fixture.sample_rate_hz,
                    fixture.frame_milliseconds,
                    fixture.padding_milliseconds,
                    fixture.minimum_speech_milliseconds,
                ),
                fixture.expected,
                "{}",
                fixture.name
            );
        }
    }
}

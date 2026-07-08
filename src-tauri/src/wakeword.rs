//! Wake-Word-Erkennung mit openWakeWord (frei, kein Lizenz-Key) - Port der
//! Android-Engine (OpenWakeWordEngine.kt) auf Windows: cpal fuer Audio,
//! ONNX Runtime (ort) statt TFLite fuer die drei Modelle.
//!
//! Pipeline (identische Parameter wie Android):
//!   Mikrofon (nativ) -> Resample 16 kHz mono
//!   -> melspectrogram.onnx (1280 Samples = 80 ms pro Aufruf)
//!   -> embedding_model.onnx (Fenster: 76 Mel-Frames, Schritt: 8)
//!   -> <wakeword>.onnx (Fenster: 16 Embeddings) -> Wahrscheinlichkeit
//!
//! Energie-Gate wie auf Android: die ML-Pipeline laeuft nur, wenn der
//! Mikrofonpegel eine Schwelle uebersteigt (+ Nachlauf); ein Pre-Roll-
//! Puffer schiebt den (oft leisen) Wortanfang nach.
//!
//! Modelle liegen in %APPDATA%/de.heimai.windows/openwakeword/:
//!   melspectrogram.onnx, embedding_model.onnx und GENAU EIN weiteres
//!   .onnx-Modell (das Wake Word, z. B. hey_jarvis_v0.1.onnx von
//!   https://github.com/dscripka/openWakeWord).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ort::session::Session;
use ort::value::Tensor;
use tauri::{AppHandle, Emitter, Manager};

const SAMPLE_RATE: usize = 16_000;
const CHUNK: usize = 1280; // 80 ms pro Melspec-Aufruf
const MEL_BINS: usize = 32;
const EMB_WINDOW: usize = 76; // Mel-Frames pro Embedding-Fenster
const EMB_STEP: usize = 8; // neues Embedding alle 8 Mel-Frames
const WW_WINDOW: usize = 16; // Embeddings pro Klassifikator-Fenster
const MEL_KEEP: usize = 120;
const EMB_KEEP: usize = 24;
const COOLDOWN: Duration = Duration::from_secs(2);
const GATE_HANGOVER: Duration = Duration::from_millis(1500);
const GATE_RMS: f32 = 0.0075; // f32-Pegel [-1,1]; Android nutzt i16-RMS ~250
const PREROLL_CHUNKS: usize = 8; // ~640 ms Vorlauf

pub struct WakeWordHandle {
    stop: Arc<AtomicBool>,
}

impl WakeWordHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub fn start(app: AppHandle, threshold: f32) -> Result<WakeWordHandle, String> {
    let models_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("openwakeword");
    let (mel_path, emb_path, ww_path) = find_models(&models_dir)?;

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();

    std::thread::Builder::new()
        .name("openwakeword".into())
        .spawn(move || {
            if let Err(error) = run_loop(app, threshold, mel_path, emb_path, ww_path, stop_thread) {
                eprintln!("Wake-Word-Schleife beendet: {error}");
            }
        })
        .map_err(|e| e.to_string())?;

    Ok(WakeWordHandle { stop })
}

fn find_models(dir: &PathBuf) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let mel = dir.join("melspectrogram.onnx");
    let emb = dir.join("embedding_model.onnx");
    if !mel.is_file() || !emb.is_file() {
        return Err(format!(
            "openWakeWord-Modelle fehlen: bitte melspectrogram.onnx, embedding_model.onnx \
             und ein Wake-Word-Modell nach {} legen",
            dir.display()
        ));
    }
    let wake = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.extension().is_some_and(|ext| ext == "onnx")
                && path.file_name() != mel.file_name()
                && path.file_name() != emb.file_name()
        })
        .ok_or_else(|| format!("kein Wake-Word-Modell (*.onnx) in {}", dir.display()))?;
    Ok((mel, emb, wake))
}

fn session(path: &PathBuf) -> Result<Session, String> {
    Session::builder()
        .and_then(|builder| builder.with_intra_threads(1))
        .and_then(|builder| builder.commit_from_file(path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn run_loop(
    app: AppHandle,
    threshold: f32,
    mel_path: PathBuf,
    emb_path: PathBuf,
    ww_path: PathBuf,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut mel_model = session(&mel_path)?;
    let mut emb_model = session(&emb_path)?;
    let mut ww_model = session(&ww_path)?;

    // ---- Audio-Eingang: natives Format des Standard-Mikrofons, Konvertierung
    // zu mono-f32 im Callback, Rest im Verarbeitungs-Thread. ----
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("kein Eingabegeraet gefunden")?;
    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let native_rate = config.sample_rate().0 as usize;
    let channels = config.channels() as usize;

    let (sender, receiver) = mpsc::channel::<Vec<f32>>();
    let stream = build_stream(&device, &config, channels, sender)?;
    stream.play().map_err(|e| e.to_string())?;

    // Zustaende der Pipeline (Namen wie im Android-Port)
    let mut resample_buffer: Vec<f32> = Vec::new();
    let mut chunk_buffer: Vec<f32> = Vec::new();
    let mut mel_frames: VecDeque<Vec<f32>> = VecDeque::new();
    let mut embeddings: VecDeque<Vec<f32>> = VecDeque::new();
    let mut new_mel_frames = 0usize;
    let mut preroll: VecDeque<Vec<f32>> = VecDeque::new();
    let mut last_voice = Instant::now() - GATE_HANGOVER * 2;
    let mut was_active = false;
    let mut cooldown_until = Instant::now();

    while !stop.load(Ordering::SeqCst) {
        let block = match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(block) => block,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        resample_buffer.extend(downsample(&block, native_rate, SAMPLE_RATE));
        chunk_buffer.append(&mut resample_buffer);

        while chunk_buffer.len() >= CHUNK {
            let chunk: Vec<f32> = chunk_buffer.drain(..CHUNK).collect();
            let now = Instant::now();

            // --- Energie-Gate (Android-Logik 1:1) ---
            let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / CHUNK as f32).sqrt();
            if rms > GATE_RMS {
                last_voice = now;
            }
            let active = now.duration_since(last_voice) < GATE_HANGOVER;
            if !active {
                preroll.push_back(chunk);
                while preroll.len() > PREROLL_CHUNKS {
                    preroll.pop_front();
                }
                if was_active {
                    mel_frames.clear();
                    embeddings.clear();
                    new_mel_frames = 0;
                    was_active = false;
                }
                continue;
            }
            if !was_active {
                was_active = true;
                for buffered in preroll.drain(..) {
                    new_mel_frames += push_mel(&mut mel_model, &buffered, &mut mel_frames)?;
                }
            }

            new_mel_frames += push_mel(&mut mel_model, &chunk, &mut mel_frames)?;
            while mel_frames.len() > MEL_KEEP {
                mel_frames.pop_front();
            }

            // --- Embeddings (alle EMB_STEP Mel-Frames, sobald 76 vorhanden) ---
            while new_mel_frames >= EMB_STEP && mel_frames.len() >= EMB_WINDOW {
                new_mel_frames -= EMB_STEP;
                embeddings.push_back(run_embedding(&mut emb_model, &mel_frames)?);
                while embeddings.len() > EMB_KEEP {
                    embeddings.pop_front();
                }
            }

            // --- Klassifikator ---
            if embeddings.len() >= WW_WINDOW && now >= cooldown_until {
                let probability = run_wakeword(&mut ww_model, &embeddings)?;
                if probability >= threshold {
                    cooldown_until = now + COOLDOWN;
                    mel_frames.clear();
                    embeddings.clear();
                    new_mel_frames = 0;
                    let _ = app.emit_to("main", "wake-word", probability);
                }
            }
        }
    }

    drop(stream);
    Ok(())
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    channels: usize,
    sender: mpsc::Sender<Vec<f32>>,
) -> Result<cpal::Stream, String> {
    let error_handler = |error| eprintln!("Audio-Stream-Fehler: {error}");
    let stream_config: cpal::StreamConfig = config.config();

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                let _ = sender.send(to_mono(data, channels));
            },
            error_handler,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                let floats: Vec<f32> = data.iter().map(|s| *s as f32 / 32768.0).collect();
                let _ = sender.send(to_mono(&floats, channels));
            },
            error_handler,
            None,
        ),
        other => return Err(format!("Sample-Format {other} nicht unterstuetzt")),
    };
    stream.map_err(|e| e.to_string())
}

fn to_mono(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }
    data.chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn downsample(input: &[f32], from_rate: usize, to_rate: usize) -> Vec<f32> {
    if from_rate == to_rate {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = (input.len() as f64 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let left = pos as usize;
            let right = (left + 1).min(input.len().saturating_sub(1));
            let frac = (pos - left as f64) as f32;
            input[left] * (1.0 - frac) + input[right] * frac
        })
        .collect()
}

/// Melspec ueber einen 1280er-Chunk; haengt normalisierte Frames an.
/// openWakeWord erwartet i16-Skalierung als f32 und normalisiert x/10+2.
fn push_mel(
    model: &mut Session,
    chunk: &[f32],
    out: &mut VecDeque<Vec<f32>>,
) -> Result<usize, String> {
    let samples: Vec<f32> = chunk.iter().map(|s| s * 32767.0).collect();
    let data = run_model(model, vec![1, CHUNK as i64], samples)?;
    let frames = data.len() / MEL_BINS;
    for frame in 0..frames {
        out.push_back(
            data[frame * MEL_BINS..(frame + 1) * MEL_BINS]
                .iter()
                .map(|value| value / 10.0 + 2.0)
                .collect(),
        );
    }
    Ok(frames)
}

/// Embedding ueber die letzten EMB_WINDOW Mel-Frames.
fn run_embedding(model: &mut Session, mel_frames: &VecDeque<Vec<f32>>) -> Result<Vec<f32>, String> {
    let start = mel_frames.len() - EMB_WINDOW;
    let mut input = Vec::with_capacity(EMB_WINDOW * MEL_BINS);
    for frame in mel_frames.iter().skip(start) {
        input.extend_from_slice(frame);
    }
    run_model(model, vec![1, EMB_WINDOW as i64, MEL_BINS as i64, 1], input)
}

/// Klassifikator ueber die letzten WW_WINDOW Embeddings -> Wahrscheinlichkeit.
fn run_wakeword(model: &mut Session, embeddings: &VecDeque<Vec<f32>>) -> Result<f32, String> {
    let emb_size = embeddings.back().map(Vec::len).unwrap_or(96);
    let start = embeddings.len() - WW_WINDOW;
    let mut input = Vec::with_capacity(WW_WINDOW * emb_size);
    for embedding in embeddings.iter().skip(start) {
        input.extend_from_slice(embedding);
    }
    let output = run_model(model, vec![1, WW_WINDOW as i64, emb_size as i64], input)?;
    Ok(output.first().copied().unwrap_or(0.0))
}

/// Ein Inferenz-Aufruf: f32-Tensor rein, flacher f32-Vektor raus.
fn run_model(model: &mut Session, shape: Vec<i64>, data: Vec<f32>) -> Result<Vec<f32>, String> {
    let tensor = Tensor::from_array((shape, data)).map_err(|e| e.to_string())?;
    let outputs = model.run(ort::inputs![tensor]).map_err(|e| e.to_string())?;
    let (_, values) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| e.to_string())?;
    Ok(values.to_vec())
}

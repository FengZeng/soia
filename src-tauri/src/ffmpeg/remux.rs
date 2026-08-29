use super::ffi;
use std::ffi::{c_char, c_int, c_void, CString};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

const AV_TIME_BASE_Q: ffi::AVRational = ffi::AVRational {
    num: 1,
    den: 1_000_000,
};
const CUSTOM_AVIO_BUFFER_SIZE: usize = 64 * 1024;

#[derive(Clone)]
pub(crate) struct StreamInput {
    pub(crate) source: crate::media_gateway::FfmpegAvioInput,
}

#[derive(Clone)]
pub(crate) struct RemuxInput {
    pub(crate) video: StreamInput,
    pub(crate) audio: StreamInput,
}

#[derive(Clone, Copy)]
pub(crate) enum ProgressiveFormat {
    MpegTs,
    FragmentedMp4,
}

/// Runs on a blocking worker and sends muxed byte chunks to the TCP task.
pub(crate) fn remux_progressive(
    input: RemuxInput,
    format: ProgressiveFormat,
    cancelled: Arc<AtomicBool>,
    packets: Sender<Vec<u8>>,
) -> Result<(), String> {
    ensure_network_ready()?;
    let output = match format {
        ProgressiveFormat::MpegTs => {
            OutputTarget::custom("mpegts", vec![("flush_packets", "1")], packets)
        }
        ProgressiveFormat::FragmentedMp4 => OutputTarget::custom(
            "mp4",
            vec![
                ("movflags", "frag_keyframe+empty_moov+default_base_moof"),
                ("frag_duration", "2000000"),
                ("flush_packets", "1"),
            ],
            packets,
        ),
    };
    remux(input, output, cancelled)
}

/// Produces a file-backed CMAF/HLS playlist and segments in `output_dir`.
pub(crate) fn remux_hls(
    input: RemuxInput,
    output_dir: &Path,
    segment_duration_seconds: u32,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    ensure_network_ready()?;
    let playlist = output_dir.join("playlist.m3u8");
    let segment_pattern = output_dir.join("segment-%05d.m4s");
    let output = OutputTarget::file(
        "hls",
        &playlist,
        vec![
            ("hls_time", segment_duration_seconds.to_string()),
            ("hls_list_size", "0".to_string()),
            ("hls_segment_type", "fmp4".to_string()),
            ("hls_fmp4_init_filename", "init.mp4".to_string()),
            (
                "hls_segment_filename",
                segment_pattern.to_string_lossy().into_owned(),
            ),
            ("hls_flags", "independent_segments+temp_file".to_string()),
        ],
    )?;
    remux(input, output, cancelled)
}

fn ensure_network_ready() -> Result<(), String> {
    static NETWORK_INIT: std::sync::OnceLock<Result<(), c_int>> = std::sync::OnceLock::new();
    NETWORK_INIT
        .get_or_init(|| {
            let result = unsafe { ffi::avformat_network_init() };
            (result >= 0).then_some(()).ok_or(result)
        })
        .as_ref()
        .map_err(|error| format!("FFmpeg network initialization failed: {}", error_message(*error)))
        .copied()
}

fn remux(input: RemuxInput, target: OutputTarget, cancelled: Arc<AtomicBool>) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        return Ok(());
    }

    let mut video = InputContext::open(input.video, ffi::AVMEDIA_TYPE_VIDEO, cancelled.clone())?;
    let mut audio = InputContext::open(input.audio, ffi::AVMEDIA_TYPE_AUDIO, cancelled.clone())?;
    let mut output = OutputContext::new(target, cancelled.clone())?;
    let video_map = output.copy_stream(&video)?;
    let audio_map = output.copy_stream(&audio)?;
    output.write_header()?;

    let mut video_packet = Packet::new()?;
    let mut audio_packet = Packet::new()?;
    let mut has_video_packet = video.read_next_selected(&mut video_packet)?;
    let mut has_audio_packet = audio.read_next_selected(&mut audio_packet)?;

    while has_video_packet || has_audio_packet {
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }

        let write_video = match (has_video_packet, has_audio_packet) {
            (true, false) => true,
            (false, true) => false,
            (true, true) => packet_time(video_packet.as_ref(), video_map.time_base)
                <= packet_time(audio_packet.as_ref(), audio_map.time_base),
            (false, false) => break,
        };

        if write_video {
            output.write_packet(video_packet.as_mut(), video_map)?;
            has_video_packet = video.read_next_selected(&mut video_packet)?;
        } else {
            output.write_packet(audio_packet.as_mut(), audio_map)?;
            has_audio_packet = audio.read_next_selected(&mut audio_packet)?;
        }
    }

    output.finish()
}

fn packet_time(packet: &ffi::AVPacket, time_base: ffi::AVRational) -> i64 {
    let timestamp = if packet.dts == i64::MIN {
        packet.pts
    } else {
        packet.dts
    };
    if timestamp == i64::MIN {
        i64::MIN
    } else {
        unsafe { ffi::av_rescale_q(timestamp, time_base, AV_TIME_BASE_Q) }
    }
}

struct InputContext {
    raw: *mut ffi::AVFormatContext,
    selected_stream_index: c_int,
    selected_stream: *mut ffi::AVStream,
    cancelled: Arc<AtomicBool>,
    custom_io: Option<CustomInputIo>,
}

struct CustomInputIo {
    raw: *mut ffi::AVIOContext,
    source: *mut crate::media_gateway::FfmpegByteStream,
}

impl InputContext {
    fn open(
        input: StreamInput,
        media_type: c_int,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let source = input.source.open(cancelled.clone())?;
        let mut raw = unsafe { ffi::avformat_alloc_context() };
        if raw.is_null() {
            return Err("FFmpeg could not allocate an input context".to_string());
        }
        unsafe {
            (*raw).interrupt_callback = ffi::AVIOInterruptCB {
                callback: Some(interrupt_callback),
                opaque: Arc::as_ptr(&cancelled).cast_mut().cast(),
            };
        }

        let custom_io = match attach_custom_input_io(raw, source) {
            Ok(custom_io) => custom_io,
            Err(error) => {
                unsafe { ffi::avformat_free_context(raw) };
                return Err(error);
            }
        };

        let mut options = ptr::null_mut();
        let result = unsafe {
            ffi::avformat_open_input(&mut raw, ptr::null(), ptr::null(), &mut options)
        };
        unsafe { ffi::av_dict_free(&mut options) };
        if result < 0 {
            unsafe { ffi::avformat_close_input(&mut raw) };
            drop_custom_input_io(custom_io);
            return Err(format!(
                "FFmpeg could not open the custom media gateway input: {}",
                error_message(result)
            ));
        }

        let result = unsafe { ffi::avformat_find_stream_info(raw, ptr::null_mut()) };
        if result < 0 {
            unsafe { ffi::avformat_close_input(&mut raw) };
            drop_custom_input_io(custom_io);
            return Err(format!(
                "FFmpeg could not read media gateway stream information: {}",
                error_message(result)
            ));
        }

        let Some((selected_stream_index, selected_stream)) =
            (unsafe { first_stream_of_type(raw, media_type) })
        else {
            unsafe { ffi::avformat_close_input(&mut raw) };
            drop_custom_input_io(custom_io);
            let label = if media_type == ffi::AVMEDIA_TYPE_VIDEO {
                "video"
            } else {
                "audio"
            };
            return Err(format!("FFmpeg found no {label} stream in the media gateway input"));
        };

        Ok(Self {
            raw,
            selected_stream_index,
            selected_stream,
            cancelled,
            custom_io: Some(custom_io),
        })
    }

    fn read_next_selected(&mut self, packet: &mut Packet) -> Result<bool, String> {
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return Ok(false);
            }
            unsafe { ffi::av_packet_unref(packet.raw) };
            let result = unsafe { ffi::av_read_frame(self.raw, packet.raw) };
            if result == ffi::AVERROR_EOF || result == ffi::AVERROR_EXIT {
                return Ok(false);
            }
            if result < 0 {
                return Err(format!("FFmpeg could not read an input packet: {}", error_message(result)));
            }
            if unsafe { (*packet.raw).stream_index } == self.selected_stream_index {
                return Ok(true);
            }
        }
    }
}

impl Drop for InputContext {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { ffi::avformat_close_input(&mut self.raw) };
        }
        if let Some(custom_io) = self.custom_io.take() {
            drop_custom_input_io(custom_io);
        }
    }
}

fn attach_custom_input_io(
    format_context: *mut ffi::AVFormatContext,
    source: crate::media_gateway::FfmpegByteStream,
) -> Result<CustomInputIo, String> {
    let buffer = unsafe { ffi::av_malloc(CUSTOM_AVIO_BUFFER_SIZE) }.cast::<u8>();
    if buffer.is_null() {
        return Err("FFmpeg could not allocate an input buffer".to_string());
    }
    let source = Box::into_raw(Box::new(source));
    let io = unsafe {
        ffi::avio_alloc_context(
            buffer,
            CUSTOM_AVIO_BUFFER_SIZE as c_int,
            0,
            source.cast(),
            Some(read_packet),
            None,
            Some(seek_input),
        )
    };
    if io.is_null() {
        unsafe {
            drop(Box::from_raw(source));
            ffi::av_free(buffer.cast());
        }
        return Err("FFmpeg could not create a custom input stream".to_string());
    }
    unsafe {
        (*format_context).pb = io;
        (*format_context).flags |= ffi::AVFMT_FLAG_CUSTOM_IO;
    }
    Ok(CustomInputIo { raw: io, source })
}

fn drop_custom_input_io(custom_io: CustomInputIo) {
    let mut io = custom_io.raw;
    unsafe {
        ffi::avio_context_free(&mut io);
        drop(Box::from_raw(custom_io.source));
    }
}

unsafe fn first_stream_of_type(
    context: *mut ffi::AVFormatContext,
    media_type: c_int,
) -> Option<(c_int, *mut ffi::AVStream)> {
    for index in 0..(*context).nb_streams as usize {
        let stream = *(*context).streams.add(index);
        if !stream.is_null()
            && !(*stream).codecpar.is_null()
            && (*(*stream).codecpar).codec_type == media_type
        {
            return Some((index as c_int, stream));
        }
    }
    None
}

#[derive(Clone, Copy)]
struct StreamMap {
    output_stream: *mut ffi::AVStream,
    output_index: c_int,
    time_base: ffi::AVRational,
}

enum OutputTarget {
    Custom {
        format: CString,
        options: Vec<(String, String)>,
        packets: Sender<Vec<u8>>,
    },
    File {
        format: CString,
        path: CString,
        options: Vec<(String, String)>,
    },
}

impl OutputTarget {
    fn custom(
        format: &str,
        options: Vec<(&str, &str)>,
        packets: Sender<Vec<u8>>,
    ) -> Self {
        Self::Custom {
            format: CString::new(format).expect("static FFmpeg format has no NUL"),
            options: options
                .into_iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            packets,
        }
    }

    fn file(format: &str, path: &Path, options: Vec<(&str, String)>) -> Result<Self, String> {
        Ok(Self::File {
            format: c_string(format, "output format")?,
            path: c_string(&path.to_string_lossy(), "output path")?,
            options: options
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        })
    }
}

struct OutputContext {
    raw: *mut ffi::AVFormatContext,
    custom_io: Option<CustomIo>,
    file_io_open: bool,
    header_written: bool,
    options: Vec<(String, String)>,
    cancelled: Arc<AtomicBool>,
}

struct CustomIo {
    raw: *mut ffi::AVIOContext,
    sink: *mut PacketSink,
}

struct PacketSink {
    cancelled: Arc<AtomicBool>,
    packets: Sender<Vec<u8>>,
}

impl OutputContext {
    fn new(target: OutputTarget, cancelled: Arc<AtomicBool>) -> Result<Self, String> {
        let (format, filename, options, custom_packets) = match target {
            OutputTarget::Custom {
                format,
                options,
                packets,
            } => (format, None, options, Some(packets)),
            OutputTarget::File {
                format,
                path,
                options,
            } => (format, Some(path), options, None),
        };
        let mut raw = ptr::null_mut();
        let result = unsafe {
            ffi::avformat_alloc_output_context2(
                &mut raw,
                ptr::null(),
                format.as_ptr(),
                filename.as_ref().map_or(ptr::null(), |path| path.as_ptr()),
            )
        };
        if result < 0 || raw.is_null() {
            return Err(format!("FFmpeg could not create {} muxer: {}", format.to_string_lossy(), error_message(result)));
        }

        let mut output = Self {
            raw,
            custom_io: None,
            file_io_open: false,
            header_written: false,
            options,
            cancelled,
        };
        if let Some(packets) = custom_packets {
            output.attach_custom_io(packets)?;
        } else if let Some(path) = filename {
            let result = unsafe { ffi::avio_open(&mut (*raw).pb, path.as_ptr(), ffi::AVIO_FLAG_WRITE) };
            if result < 0 {
                return Err(format!("FFmpeg could not open output {}: {}", path.to_string_lossy(), error_message(result)));
            }
            output.file_io_open = true;
        }
        Ok(output)
    }

    fn attach_custom_io(&mut self, packets: Sender<Vec<u8>>) -> Result<(), String> {
        let buffer = unsafe { ffi::av_malloc(CUSTOM_AVIO_BUFFER_SIZE) }.cast::<u8>();
        if buffer.is_null() {
            return Err("FFmpeg could not allocate an output buffer".to_string());
        }
        let sink = Box::into_raw(Box::new(PacketSink {
            cancelled: self.cancelled.clone(),
            packets,
        }));
        let io = unsafe {
            ffi::avio_alloc_context(
                buffer,
                CUSTOM_AVIO_BUFFER_SIZE as c_int,
                1,
                sink.cast(),
                None,
                Some(write_packet),
                None,
            )
        };
        if io.is_null() {
            unsafe {
                drop(Box::from_raw(sink));
                ffi::av_free(buffer.cast());
            }
            return Err("FFmpeg could not create a custom output stream".to_string());
        }
        unsafe {
            (*self.raw).pb = io;
            (*self.raw).flags |= ffi::AVFMT_FLAG_CUSTOM_IO;
        }
        self.custom_io = Some(CustomIo { raw: io, sink });
        Ok(())
    }

    fn copy_stream(&mut self, input: &InputContext) -> Result<StreamMap, String> {
        let output_stream = unsafe { ffi::avformat_new_stream(self.raw, ptr::null()) };
        if output_stream.is_null() {
            return Err("FFmpeg could not create an output stream".to_string());
        }
        let result = unsafe {
            ffi::avcodec_parameters_copy(
                (*output_stream).codecpar,
                (*input.selected_stream).codecpar,
            )
        };
        if result < 0 {
            return Err(format!("FFmpeg could not copy stream parameters: {}", error_message(result)));
        }
        unsafe { (*output_stream).time_base = (*input.selected_stream).time_base };
        Ok(StreamMap {
            output_stream,
            output_index: unsafe { (*output_stream).index },
            time_base: unsafe { (*input.selected_stream).time_base },
        })
    }

    fn write_header(&mut self) -> Result<(), String> {
        let mut options = dictionary(&self.options)?;
        let result = unsafe { ffi::avformat_write_header(self.raw, &mut options) };
        unsafe { ffi::av_dict_free(&mut options) };
        if result < 0 {
            return Err(format!("FFmpeg could not write the output header: {}", error_message(result)));
        }
        self.header_written = true;
        Ok(())
    }

    fn write_packet(&mut self, packet: *mut ffi::AVPacket, mapping: StreamMap) -> Result<(), String> {
        unsafe {
            (*packet).stream_index = mapping.output_index;
            ffi::av_packet_rescale_ts(packet, mapping.time_base, (*mapping.output_stream).time_base);
        }
        let result = unsafe { ffi::av_interleaved_write_frame(self.raw, packet) };
        if result < 0 {
            unsafe { ffi::av_packet_unref(packet) };
            if self.cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            return Err(format!("FFmpeg could not write an output packet: {}", error_message(result)));
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        if !self.header_written {
            return Ok(());
        }
        let result = unsafe { ffi::av_write_trailer(self.raw) };
        self.header_written = false;
        if result < 0 && !self.cancelled.load(Ordering::Acquire) {
            return Err(format!("FFmpeg could not finalize the output: {}", error_message(result)));
        }
        Ok(())
    }
}

impl Drop for OutputContext {
    fn drop(&mut self) {
        if self.header_written {
            unsafe { ffi::av_write_trailer(self.raw) };
        }
        if let Some(custom_io) = self.custom_io.take() {
            let mut io = custom_io.raw;
            unsafe {
                ffi::avio_context_free(&mut io);
                drop(Box::from_raw(custom_io.sink));
            }
        } else if self.file_io_open {
            unsafe { ffi::avio_closep(&mut (*self.raw).pb) };
        }
        if !self.raw.is_null() {
            unsafe { ffi::avformat_free_context(self.raw) };
        }
    }
}

struct Packet {
    raw: *mut ffi::AVPacket,
}

impl Packet {
    fn new() -> Result<Self, String> {
        let raw = unsafe { ffi::av_packet_alloc() };
        (!raw.is_null())
            .then_some(Self { raw })
            .ok_or_else(|| "FFmpeg could not allocate a packet".to_string())
    }

    fn as_ref(&self) -> &ffi::AVPacket {
        unsafe { &*self.raw }
    }

    fn as_mut(&mut self) -> *mut ffi::AVPacket {
        self.raw
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { ffi::av_packet_free(&mut self.raw) };
        }
    }
}

unsafe extern "C" fn interrupt_callback(opaque: *mut c_void) -> c_int {
    let cancelled = &*opaque.cast::<AtomicBool>();
    cancelled.load(Ordering::Acquire) as c_int
}

unsafe extern "C" fn read_packet(opaque: *mut c_void, data: *mut u8, size: c_int) -> c_int {
    if opaque.is_null() || data.is_null() || size <= 0 {
        return ffi::AVERROR_EXIT;
    }
    let source = &mut *opaque.cast::<crate::media_gateway::FfmpegByteStream>();
    let buffer = std::slice::from_raw_parts_mut(data, size as usize);
    match source.read(buffer) {
        Ok(0) => ffi::AVERROR_EOF,
        Ok(read) => read.min(c_int::MAX as usize) as c_int,
        Err(_) => ffi::AVERROR_EXIT,
    }
}

unsafe extern "C" fn seek_input(opaque: *mut c_void, offset: i64, whence: c_int) -> i64 {
    if opaque.is_null() {
        return -1;
    }
    let source = &mut *opaque.cast::<crate::media_gateway::FfmpegByteStream>();
    source.seek(offset, whence).unwrap_or(-1)
}

unsafe extern "C" fn write_packet(
    opaque: *mut c_void,
    data: *const u8,
    size: c_int,
) -> c_int {
    if opaque.is_null() || data.is_null() || size < 0 {
        return ffi::AVERROR_EXIT;
    }
    let sink = &*opaque.cast::<PacketSink>();
    if sink.cancelled.load(Ordering::Acquire) {
        return ffi::AVERROR_EXIT;
    }
    let bytes = std::slice::from_raw_parts(data, size as usize).to_vec();
    sink.packets
        .blocking_send(bytes)
        .map(|_| size)
        .unwrap_or(ffi::AVERROR_EXIT)
}

fn dictionary(values: &[(String, String)]) -> Result<*mut ffi::AVDictionary, String> {
    let mut options = ptr::null_mut();
    for (key, value) in values {
        set_dictionary_value(&mut options, key, value)?;
    }
    Ok(options)
}

fn set_dictionary_value(
    dictionary: &mut *mut ffi::AVDictionary,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let key = c_string(key, "FFmpeg option name")?;
    let value = c_string(value, "FFmpeg option value")?;
    let result = unsafe { ffi::av_dict_set(dictionary, key.as_ptr(), value.as_ptr(), 0) };
    if result < 0 {
        return Err(format!("FFmpeg could not set option {}: {}", key.to_string_lossy(), error_message(result)));
    }
    Ok(())
}

fn c_string(value: &str, label: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| format!("{label} contains a NUL byte"))
}

fn error_message(error: c_int) -> String {
    let mut buffer = [0 as c_char; 256];
    let result = unsafe { ffi::av_strerror(error, buffer.as_mut_ptr(), buffer.len()) };
    if result < 0 {
        return format!("FFmpeg error {error}");
    }
    unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::ensure_network_ready;

    #[test]
    fn initializes_the_bundled_ffmpeg_network_api() {
        ensure_network_ready().expect("bundled FFmpeg network API should initialize");
    }
}

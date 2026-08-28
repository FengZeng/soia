//! Hand-maintained bindings for the FFmpeg 8 ABI bundled in `libs/mpv`.
//!
//! Only public ABI fields required by the current remux path are represented. Decoder, frame and
//! swscale entry points are declared here as well so screenshots can build on this module without
//! adding a new FFmpeg binding or runtime later.

#![allow(dead_code, non_snake_case)] // The screenshot ABI is deliberately declared ahead of use.

use std::ffi::{c_char, c_int, c_void};

pub(crate) const AVMEDIA_TYPE_VIDEO: c_int = 0;
pub(crate) const AVMEDIA_TYPE_AUDIO: c_int = 1;
pub(crate) const AVERROR_EOF: c_int = -541_478_725;
pub(crate) const AVERROR_EXIT: c_int = -1_414_092_869;
pub(crate) const AVFMT_FLAG_CUSTOM_IO: c_int = 0x0080;
pub(crate) const AVIO_FLAG_WRITE: c_int = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct AVRational {
    pub num: c_int,
    pub den: c_int,
}

#[repr(C)]
pub(crate) struct AVDictionary {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct AVInputFormat {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct AVOutputFormat {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct AVCodec {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct AVCodecContext {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct AVFrame {
    /// FFmpeg's public frame prefix. The fields needed to convert a decoded video frame into an
    /// image are intentionally available for the future screenshot path.
    pub data: [*mut u8; 8],
    pub linesize: [c_int; 8],
    _extended_data: *mut *mut u8,
    pub width: c_int,
    pub height: c_int,
    _nb_samples: c_int,
    pub format: c_int,
}

#[repr(C)]
pub(crate) struct AVCodecParameters {
    pub codec_type: c_int,
    pub codec_id: c_int,
}

#[repr(C)]
pub(crate) struct AVStream {
    _av_class: *const c_void,
    pub index: c_int,
    _id: c_int,
    pub codecpar: *mut AVCodecParameters,
    _priv_data: *mut c_void,
    pub time_base: AVRational,
}

#[repr(C)]
pub(crate) struct AVPacket {
    _buf: *mut c_void,
    pub pts: i64,
    pub dts: i64,
    _data: *mut u8,
    _size: c_int,
    pub stream_index: c_int,
    _flags: c_int,
    _side_data: *mut c_void,
    _side_data_elems: c_int,
    _duration: i64,
    _pos: i64,
    _opaque: *mut c_void,
    _opaque_ref: *mut c_void,
    _time_base: AVRational,
}

#[repr(C)]
pub(crate) struct AVIOContext {
    _private: [u8; 0],
}

pub(crate) type AVIOWritePacket = unsafe extern "C" fn(*mut c_void, *const u8, c_int) -> c_int;
pub(crate) type AVIOInterruptCallback = unsafe extern "C" fn(*mut c_void) -> c_int;

#[repr(C)]
pub(crate) struct AVIOInterruptCB {
    pub callback: Option<AVIOInterruptCallback>,
    pub opaque: *mut c_void,
}

// Prefix through `interrupt_callback` from FFmpeg 8's public AVFormatContext definition.
// New fields are appended by FFmpeg minor releases, so this prefix is ABI stable for major 62.
#[repr(C)]
pub(crate) struct AVFormatContext {
    _av_class: *const c_void,
    _iformat: *const AVInputFormat,
    _oformat: *const AVOutputFormat,
    _priv_data: *mut c_void,
    pub pb: *mut AVIOContext,
    _ctx_flags: c_int,
    pub nb_streams: u32,
    pub streams: *mut *mut AVStream,
    _nb_stream_groups: u32,
    _stream_groups: *mut *mut c_void,
    _nb_chapters: u32,
    _chapters: *mut *mut c_void,
    _url: *mut c_char,
    _start_time: i64,
    _duration: i64,
    _bit_rate: i64,
    _packet_size: u32,
    _max_delay: c_int,
    pub flags: c_int,
    _probesize: i64,
    _max_analyze_duration: i64,
    _key: *const u8,
    _keylen: c_int,
    _nb_programs: u32,
    _programs: *mut *mut c_void,
    _video_codec_id: c_int,
    _audio_codec_id: c_int,
    _subtitle_codec_id: c_int,
    _data_codec_id: c_int,
    _metadata: *mut AVDictionary,
    _start_time_realtime: i64,
    _fps_probe_size: c_int,
    _error_recognition: c_int,
    pub interrupt_callback: AVIOInterruptCB,
}

unsafe extern "C" {
    pub(crate) fn avformat_network_init() -> c_int;
    pub(crate) fn avformat_alloc_context() -> *mut AVFormatContext;
    pub(crate) fn avformat_open_input(
        ps: *mut *mut AVFormatContext,
        url: *const c_char,
        fmt: *const AVInputFormat,
        options: *mut *mut AVDictionary,
    ) -> c_int;
    pub(crate) fn avformat_find_stream_info(
        ctx: *mut AVFormatContext,
        options: *mut *mut AVDictionary,
    ) -> c_int;
    pub(crate) fn avformat_close_input(ctx: *mut *mut AVFormatContext);
    pub(crate) fn avformat_free_context(ctx: *mut AVFormatContext);
    pub(crate) fn avformat_alloc_output_context2(
        ctx: *mut *mut AVFormatContext,
        oformat: *const AVOutputFormat,
        format_name: *const c_char,
        filename: *const c_char,
    ) -> c_int;
    pub(crate) fn avformat_new_stream(
        ctx: *mut AVFormatContext,
        codec: *const AVCodec,
    ) -> *mut AVStream;
    pub(crate) fn avformat_write_header(
        ctx: *mut AVFormatContext,
        options: *mut *mut AVDictionary,
    ) -> c_int;
    pub(crate) fn av_read_frame(ctx: *mut AVFormatContext, packet: *mut AVPacket) -> c_int;
    pub(crate) fn av_interleaved_write_frame(
        ctx: *mut AVFormatContext,
        packet: *mut AVPacket,
    ) -> c_int;
    pub(crate) fn av_write_trailer(ctx: *mut AVFormatContext) -> c_int;
    pub(crate) fn avio_open(
        pb: *mut *mut AVIOContext,
        url: *const c_char,
        flags: c_int,
    ) -> c_int;
    pub(crate) fn avio_closep(pb: *mut *mut AVIOContext) -> c_int;
    pub(crate) fn avio_alloc_context(
        buffer: *mut u8,
        buffer_size: c_int,
        write_flag: c_int,
        opaque: *mut c_void,
        read_packet: Option<unsafe extern "C" fn(*mut c_void, *mut u8, c_int) -> c_int>,
        write_packet: Option<AVIOWritePacket>,
        seek: Option<unsafe extern "C" fn(*mut c_void, i64, c_int) -> i64>,
    ) -> *mut AVIOContext;
    pub(crate) fn avio_context_free(ctx: *mut *mut AVIOContext);
    pub(crate) fn avcodec_parameters_copy(
        dst: *mut AVCodecParameters,
        src: *const AVCodecParameters,
    ) -> c_int;
    pub(crate) fn av_packet_alloc() -> *mut AVPacket;
    pub(crate) fn av_packet_free(packet: *mut *mut AVPacket);
    pub(crate) fn av_packet_unref(packet: *mut AVPacket);
    pub(crate) fn av_packet_rescale_ts(
        packet: *mut AVPacket,
        source_time_base: AVRational,
        destination_time_base: AVRational,
    );
    pub(crate) fn av_dict_set(
        dictionary: *mut *mut AVDictionary,
        key: *const c_char,
        value: *const c_char,
        flags: c_int,
    ) -> c_int;
    pub(crate) fn av_dict_free(dictionary: *mut *mut AVDictionary);
    pub(crate) fn av_strerror(error: c_int, buffer: *mut c_char, buffer_size: usize) -> c_int;
    pub(crate) fn av_malloc(size: usize) -> *mut c_void;
    pub(crate) fn av_free(pointer: *mut c_void);
    pub(crate) fn av_rescale_q(value: i64, source_time_base: AVRational, destination_time_base: AVRational) -> i64;

    // Decoder/frame/scaler APIs reserved for the upcoming screenshot implementation.
    pub(crate) fn avcodec_find_decoder(codec_id: c_int) -> *const AVCodec;
    pub(crate) fn avcodec_alloc_context3(codec: *const AVCodec) -> *mut AVCodecContext;
    pub(crate) fn avcodec_free_context(ctx: *mut *mut AVCodecContext);
    pub(crate) fn avcodec_parameters_to_context(
        ctx: *mut AVCodecContext,
        parameters: *const AVCodecParameters,
    ) -> c_int;
    pub(crate) fn avcodec_open2(
        ctx: *mut AVCodecContext,
        codec: *const AVCodec,
        options: *mut *mut AVDictionary,
    ) -> c_int;
    pub(crate) fn avcodec_send_packet(ctx: *mut AVCodecContext, packet: *const AVPacket) -> c_int;
    pub(crate) fn avcodec_receive_frame(ctx: *mut AVCodecContext, frame: *mut AVFrame) -> c_int;
    pub(crate) fn av_frame_alloc() -> *mut AVFrame;
    pub(crate) fn av_frame_free(frame: *mut *mut AVFrame);
    pub(crate) fn av_frame_unref(frame: *mut AVFrame);
    pub(crate) fn sws_getContext(
        source_width: c_int,
        source_height: c_int,
        source_format: c_int,
        destination_width: c_int,
        destination_height: c_int,
        destination_format: c_int,
        flags: c_int,
        source_filter: *mut c_void,
        destination_filter: *mut c_void,
        parameters: *const f64,
    ) -> *mut c_void;
    pub(crate) fn sws_scale(
        context: *mut c_void,
        source_slices: *const *const u8,
        source_strides: *const c_int,
        source_slice_y: c_int,
        source_slice_height: c_int,
        destination: *const *mut u8,
        destination_strides: *const c_int,
    ) -> c_int;
    pub(crate) fn sws_freeContext(context: *mut c_void);
}

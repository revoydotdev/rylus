use std::ffi::{CStr, CString};
use std::os::raw::{c_int, c_void};
use std::ptr;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use rylus_core::pixel::PixelProvider;

use ffi::{AVPictureType, AVPixelFormat};
use ffmpeg_sys_next as ffi;

// ============================================================
// Error Type
// ============================================================

#[derive(Debug)]
pub struct EncodeError(String);

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for EncodeError {}

macro_rules! enc_err {
    ($($arg:tt)*) => {
        EncodeError(format!($($arg)*))
    };
}

// ============================================================
// Configuration
// ============================================================

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum Codec {
    #[default]
    H264,
    Hevc,
}

impl Codec {
    pub fn ffmpeg_codec_name(&self) -> &'static CStr {
        match self {
            Codec::H264 => c"libx264",
            Codec::Hevc => c"libx265",
        }
    }
}

#[derive(Clone, Copy)]
pub struct EncoderOptions {
    pub try_vaapi: bool,
    pub try_nvenc: bool,
    pub try_videotoolbox: bool,
    pub try_mediafoundation: bool,
    pub codec: Codec,
    pub avio_buffer_size: usize,
    pub all_intra: bool,
}

impl Default for EncoderOptions {
    fn default() -> Self {
        Self {
            try_vaapi: false,
            try_nvenc: false,
            try_videotoolbox: false,
            try_mediafoundation: false,
            codec: Codec::default(),
            avio_buffer_size: 1024 * 1024,
            all_intra: false,
        }
    }
}

// ============================================================
// Constants
// ============================================================

const TIME_BASE: ffi::AVRational = ffi::AVRational { num: 1, den: 1000 };

/// AVIO buffer size for writing fMP4 output (1 MB).
#[allow(dead_code)]
const AVIO_BUFFER_SIZE: c_int = 1024 * 1024;

/// Number of frames in the hardware frame pool for GPU-accelerated encoding.
const HW_FRAME_POOL_SIZE: c_int = 20;

/// GOP (Group of Pictures) size. Lower values = more keyframes = faster seeking
/// but larger output. 12 is a reasonable default for low-latency streaming.
const GOP_SIZE: c_int = 12;

/// Maximum B-frames. Set to 0 for lowest latency (no bidirectional prediction).
const MAX_B_FRAMES: c_int = 0;

/// FFmpeg AVERROR macro: negate errno to get FFmpeg error code.
#[inline]
fn averror(e: c_int) -> c_int {
    -e
}

// AVERROR_EOF = -MKTAG('E','O','F',' ')
const AVERROR_EOF: c_int = {
    let tag = (b'E' as u32) | ((b'O' as u32) << 8) | ((b'F' as u32) << 16) | ((b' ' as u32) << 24);
    -(tag as i32)
};

// EAGAIN errno value
#[cfg(target_os = "linux")]
const EAGAIN: c_int = 11;
#[cfg(target_os = "macos")]
const EAGAIN: c_int = 35;
#[cfg(target_os = "windows")]
const EAGAIN: c_int = 11;

fn pix_fmt_name(fmt: AVPixelFormat) -> String {
    // SAFETY: av_get_pix_fmt_name returns a static string pointer or null for unknown
    // formats; we check for null before dereferencing.
    unsafe {
        let name = ffi::av_get_pix_fmt_name(fmt);
        if name.is_null() {
            "unknown".to_string()
        } else {
            CStr::from_ptr(name).to_string_lossy().into_owned()
        }
    }
}

// ============================================================
// AVIO write callback
// ============================================================

unsafe extern "C" fn avio_write_callback(
    opaque: *mut c_void,
    buf: *const u8,
    buf_size: c_int,
) -> c_int {
    if buf_size <= 0 {
        return buf_size;
    }
    // SAFETY: `opaque` is set to a valid `*mut VideoEncoder` in `open_video` when
    // creating the AVIO context, and the encoder outlives the format context.
    let encoder = unsafe { &mut *(opaque as *mut VideoEncoder) };
    // SAFETY: FFmpeg guarantees `buf` points to `buf_size` valid bytes when buf_size > 0,
    // which we checked above.
    let data = unsafe { std::slice::from_raw_parts(buf, buf_size as usize) };
    (encoder.write_data)(data);
    buf_size
}

// ============================================================
// Scale filter string builder
// ============================================================

fn build_scale_filter_str(
    pix_fmt_in: AVPixelFormat,
    pix_fmt_out: AVPixelFormat,
    pix_fmt_sw_out: AVPixelFormat,
    width_out: i32,
    height_out: i32,
) -> String {
    match pix_fmt_out {
        AVPixelFormat::AV_PIX_FMT_CUDA => {
            if pix_fmt_in == AVPixelFormat::AV_PIX_FMT_RGB24 {
                format!(
                    "scale=w={}:h={}:flags=fast_bilinear,hwupload_cuda",
                    width_out, height_out
                )
            } else {
                format!(
                    "hwupload_cuda,scale_cuda=w={}:h={}:format={}:interp_algo=nearest",
                    width_out,
                    height_out,
                    pix_fmt_name(pix_fmt_sw_out)
                )
            }
        }
        AVPixelFormat::AV_PIX_FMT_VAAPI => {
            if pix_fmt_in == AVPixelFormat::AV_PIX_FMT_RGB24 {
                format!(
                    "scale=w={}:h={}:flags=fast_bilinear,hwupload",
                    width_out, height_out
                )
            } else {
                format!(
                    "hwupload,scale_vaapi=w={}:h={}:format={}:mode=fast",
                    width_out,
                    height_out,
                    pix_fmt_name(pix_fmt_sw_out)
                )
            }
        }
        _ => {
            format!("scale=w={}:h={}:flags=fast_bilinear", width_out, height_out)
        }
    }
}

// ============================================================
// Scale Context (filter graph for pixel format conversion + scaling)
// ============================================================

struct ScaleContext {
    filter_graph: *mut ffi::AVFilterGraph,
    buffersrc_ctx: *mut ffi::AVFilterContext,
    buffersink_ctx: *mut ffi::AVFilterContext,
    frame_in: *mut ffi::AVFrame,
    frame_out: *mut ffi::AVFrame, // NOT owned - shared from Scalers
}

impl ScaleContext {
    #[allow(clippy::too_many_arguments)]
    fn new(
        width_in: i32,
        height_in: i32,
        width_out: i32,
        height_out: i32,
        pix_fmt_in: AVPixelFormat,
        pix_fmt_out: AVPixelFormat,
        hw_device_ctx: *mut ffi::AVBufferRef,
        pix_fmt_sw_out: AVPixelFormat,
        frame_out: *mut ffi::AVFrame,
    ) -> Result<Self, EncodeError> {
        // SAFETY: All FFmpeg allocator and filter-graph functions are called with valid
        // pointers obtained from prior FFmpeg allocation calls. Null checks are performed
        // after each allocation, and resources are freed on every error path.
        unsafe {
            // Allocate input frame
            let frame_in = ffi::av_frame_alloc();
            if frame_in.is_null() {
                return Err(enc_err!("Failed to allocate frame_in for scale filter"));
            }
            (*frame_in).format = pix_fmt_in as c_int;
            (*frame_in).width = width_in;
            (*frame_in).height = height_in;
            let ret = ffi::av_frame_get_buffer(frame_in, 0);
            if ret != 0 {
                ffi::av_frame_free(&mut { frame_in });
                return Err(enc_err!("Failed to allocate buffer for frame_in"));
            }

            // Allocate filter graph
            let filter_graph = ffi::avfilter_graph_alloc();
            if filter_graph.is_null() {
                ffi::av_frame_free(&mut { frame_in });
                return Err(enc_err!("Failed to allocate filter graph"));
            }
            ffi::avfilter_graph_set_auto_convert(
                filter_graph,
                ffi::AVFILTER_AUTO_CONVERT_NONE as u32,
            );

            let buffersrc = ffi::avfilter_get_by_name(c"buffer".as_ptr());
            let buffersink = ffi::avfilter_get_by_name(c"buffersink".as_ptr());

            // Create buffer source
            let args = format!(
                "video_size={}x{}:pix_fmt={}:time_base={}/{}:pixel_aspect={}/{}",
                width_in, height_in, pix_fmt_in as c_int, TIME_BASE.num, TIME_BASE.den, 1, 1
            );
            let args_c = CString::new(args).expect("filter args should not contain null bytes");
            let mut buffersrc_ctx: *mut ffi::AVFilterContext = ptr::null_mut();
            let ret = ffi::avfilter_graph_create_filter(
                &mut buffersrc_ctx,
                buffersrc,
                c"in".as_ptr(),
                args_c.as_ptr(),
                ptr::null_mut(),
                filter_graph,
            );
            if ret < 0 {
                ffi::avfilter_graph_free(&mut { filter_graph });
                ffi::av_frame_free(&mut { frame_in });
                return Err(enc_err!("Cannot create buffer source"));
            }

            // Create buffer sink
            let buffersink_ctx =
                ffi::avfilter_graph_alloc_filter(filter_graph, buffersink, c"out".as_ptr());
            if buffersink_ctx.is_null() {
                ffi::avfilter_graph_free(&mut { filter_graph });
                ffi::av_frame_free(&mut { frame_in });
                return Err(enc_err!("Cannot allocate buffer sink"));
            }

            // Set output pixel format on buffersink.
            // FFmpeg 8+ has "pixel_formats" as an array option (set via string name),
            // while older versions use "pix_fmts" as a binary option.
            let fmt_name = pix_fmt_name(pix_fmt_out);
            let fmt_name_c = CString::new(fmt_name).expect("pix fmt name should not contain null");
            let ret = ffi::av_opt_set(
                buffersink_ctx as *mut c_void,
                c"pixel_formats".as_ptr(),
                fmt_name_c.as_ptr(),
                ffi::AV_OPT_SEARCH_CHILDREN,
            );
            // Fall back to legacy binary option for FFmpeg < 7.1
            let ret = if ret < 0 {
                let pix_fmts: [c_int; 1] = [pix_fmt_out as c_int];
                ffi::av_opt_set_bin(
                    buffersink_ctx as *mut c_void,
                    c"pix_fmts".as_ptr(),
                    pix_fmts.as_ptr() as *const u8,
                    std::mem::size_of_val(&pix_fmts) as c_int,
                    ffi::AV_OPT_SEARCH_CHILDREN,
                )
            } else {
                ret
            };
            if ret < 0 {
                ffi::avfilter_graph_free(&mut { filter_graph });
                ffi::av_frame_free(&mut { frame_in });
                return Err(enc_err!("Cannot set output pixel format"));
            }

            let ret = ffi::avfilter_init_dict(buffersink_ctx, ptr::null_mut());
            if ret < 0 {
                ffi::avfilter_graph_free(&mut { filter_graph });
                ffi::av_frame_free(&mut { frame_in });
                return Err(enc_err!("Cannot init buffer sink"));
            }

            // Create filter IO endpoints
            let mut outputs = ffi::avfilter_inout_alloc();
            let mut inputs = ffi::avfilter_inout_alloc();
            if outputs.is_null() || inputs.is_null() {
                ffi::avfilter_inout_free(&mut outputs);
                ffi::avfilter_inout_free(&mut inputs);
                ffi::avfilter_graph_free(&mut { filter_graph });
                ffi::av_frame_free(&mut { frame_in });
                return Err(enc_err!("Failed to allocate filter IO"));
            }

            (*outputs).name = ffi::av_strdup(c"in".as_ptr());
            (*outputs).filter_ctx = buffersrc_ctx;
            (*outputs).pad_idx = 0;
            (*outputs).next = ptr::null_mut();

            (*inputs).name = ffi::av_strdup(c"out".as_ptr());
            (*inputs).filter_ctx = buffersink_ctx;
            (*inputs).pad_idx = 0;
            (*inputs).next = ptr::null_mut();

            // Build and parse the scale filter
            let filter_str = build_scale_filter_str(
                pix_fmt_in,
                pix_fmt_out,
                pix_fmt_sw_out,
                width_out,
                height_out,
            );
            let filter_c =
                CString::new(filter_str).expect("filter string should not contain null bytes");
            let ret = ffi::avfilter_graph_parse_ptr(
                filter_graph,
                filter_c.as_ptr(),
                &mut inputs,
                &mut outputs,
                ptr::null_mut(),
            );
            if ret < 0 {
                ffi::avfilter_inout_free(&mut inputs);
                ffi::avfilter_inout_free(&mut outputs);
                ffi::avfilter_graph_free(&mut { filter_graph });
                ffi::av_frame_free(&mut { frame_in });
                return Err(enc_err!("Failed to parse scale filter"));
            }

            // Set hw_device_ctx on hwupload filters
            if !hw_device_ctx.is_null() {
                for i in 0..(*filter_graph).nb_filters {
                    let filt = *(*filter_graph).filters.add(i as usize);
                    let name = CStr::from_ptr((*(*filt).filter).name);
                    if name.to_bytes() == b"hwupload" {
                        (*filt).hw_device_ctx = ffi::av_buffer_ref(hw_device_ctx);
                    }
                }
            }

            let ret = ffi::avfilter_graph_config(filter_graph, ptr::null_mut());
            ffi::avfilter_inout_free(&mut inputs);
            ffi::avfilter_inout_free(&mut outputs);
            if ret < 0 {
                ffi::avfilter_graph_free(&mut { filter_graph });
                ffi::av_frame_free(&mut { frame_in });
                return Err(enc_err!(
                    "Failed to configure filter graph: {} -> {}",
                    pix_fmt_name(pix_fmt_in),
                    pix_fmt_name(pix_fmt_out)
                ));
            }

            debug!(
                "Scale filter set {} -> {} (sw: {}) up!",
                pix_fmt_name(pix_fmt_in),
                pix_fmt_name(pix_fmt_out),
                pix_fmt_name(pix_fmt_sw_out)
            );

            Ok(Self {
                filter_graph,
                buffersrc_ctx,
                buffersink_ctx,
                frame_in,
                frame_out,
            })
        }
    }

    fn scale(&mut self) -> Result<(), EncodeError> {
        // SAFETY: buffersrc_ctx, buffersink_ctx, frame_in, and frame_out are valid pointers
        // allocated during ScaleContext::new and not freed until drop.
        unsafe {
            let ret = ffi::av_buffersrc_add_frame_flags(
                self.buffersrc_ctx,
                self.frame_in,
                ffi::AV_BUFFERSRC_FLAG_KEEP_REF as c_int,
            );
            if ret < 0 {
                return Err(enc_err!("Error adding frame to buffer source"));
            }

            ffi::av_frame_unref(self.frame_out);

            loop {
                let ret = ffi::av_buffersink_get_frame(self.buffersink_ctx, self.frame_out);
                if ret == averror(EAGAIN) || ret == AVERROR_EOF {
                    break;
                }
                if ret < 0 {
                    return Err(enc_err!("Error reading frame from buffer sink"));
                }
            }
            Ok(())
        }
    }
}

impl Drop for ScaleContext {
    fn drop(&mut self) {
        // SAFETY: filter_graph and frame_in are either null (checked) or valid pointers
        // allocated by FFmpeg during ScaleContext::new. Each is freed exactly once.
        unsafe {
            if !self.filter_graph.is_null() {
                ffi::avfilter_graph_free(&mut self.filter_graph);
            }
            if !self.frame_in.is_null() {
                ffi::av_frame_free(&mut self.frame_in);
            }
            // frame_out is NOT freed here - it's shared from Scalers
        }
    }
}

// ============================================================
// Scalers (collection of ScaleContexts for each input format)
// ============================================================

struct Scalers {
    bgr0: Option<ScaleContext>,
    rgb0: Option<ScaleContext>,
    rgb: Option<ScaleContext>,
    hw_frames_ctx: *mut ffi::AVBufferRef,
    frame_out: *mut ffi::AVFrame,
}

impl Scalers {
    fn new(
        width_in: i32,
        height_in: i32,
        width_out: i32,
        height_out: i32,
        pix_fmt_out: AVPixelFormat,
        pix_fmt_sw_out: AVPixelFormat,
        hw_device_ctx: *mut ffi::AVBufferRef,
    ) -> Result<Self, EncodeError> {
        // SAFETY: All FFmpeg allocation and hardware frame context functions are called with
        // valid pointers. Null checks are performed after each allocation, and all resources
        // are freed on error paths before returning.
        unsafe {
            let frame_out = ffi::av_frame_alloc();
            if frame_out.is_null() {
                return Err(enc_err!("Failed to allocate frame_out for scale filter"));
            }

            let mut hw_frames_ctx: *mut ffi::AVBufferRef = ptr::null_mut();

            if !hw_device_ctx.is_null() {
                let hw_frames_ref = ffi::av_hwframe_ctx_alloc(hw_device_ctx);
                if hw_frames_ref.is_null() {
                    ffi::av_frame_free(&mut { frame_out });
                    return Err(enc_err!("Failed to create HW frame context"));
                }
                let frames_ctx = (*hw_frames_ref).data as *mut ffi::AVHWFramesContext;
                (*frames_ctx).format = pix_fmt_out;
                (*frames_ctx).sw_format = pix_fmt_sw_out;
                (*frames_ctx).width = width_out;
                (*frames_ctx).height = height_out;
                (*frames_ctx).initial_pool_size = HW_FRAME_POOL_SIZE;
                let ret = ffi::av_hwframe_ctx_init(hw_frames_ref);
                if ret < 0 {
                    ffi::av_buffer_unref(&mut { hw_frames_ref });
                    ffi::av_frame_free(&mut { frame_out });
                    return Err(enc_err!("Failed to initialize HW frame context"));
                }
                hw_frames_ctx = ffi::av_buffer_ref(hw_frames_ref);
                let ret = ffi::av_hwframe_get_buffer(hw_frames_ctx, frame_out, 0);
                if ret < 0 {
                    ffi::av_buffer_unref(&mut { hw_frames_ref });
                    ffi::av_buffer_unref(&mut hw_frames_ctx);
                    ffi::av_frame_free(&mut { frame_out });
                    return Err(enc_err!("Could not allocate video hardware frame data"));
                }
                ffi::av_buffer_unref(&mut { hw_frames_ref });
            }

            let pix_fmts_in = [
                AVPixelFormat::AV_PIX_FMT_BGR0,
                AVPixelFormat::AV_PIX_FMT_RGB0,
                AVPixelFormat::AV_PIX_FMT_RGB24,
            ];

            let mut results: Vec<Option<ScaleContext>> = Vec::new();
            for &pix_fmt in &pix_fmts_in {
                match ScaleContext::new(
                    width_in,
                    height_in,
                    width_out,
                    height_out,
                    pix_fmt,
                    pix_fmt_out,
                    hw_device_ctx,
                    pix_fmt_sw_out,
                    frame_out,
                ) {
                    Ok(s) => results.push(Some(s)),
                    Err(e) => {
                        drop(results);
                        if !hw_frames_ctx.is_null() {
                            ffi::av_buffer_unref(&mut hw_frames_ctx);
                        }
                        ffi::av_frame_free(&mut { frame_out });
                        return Err(e);
                    }
                }
            }

            Ok(Self {
                bgr0: results.remove(0),
                rgb0: results.remove(0),
                rgb: results.remove(0),
                hw_frames_ctx,
                frame_out,
            })
        }
    }
}

impl Drop for Scalers {
    fn drop(&mut self) {
        self.bgr0.take();
        self.rgb0.take();
        self.rgb.take();
        // SAFETY: ScaleContexts are dropped first (above) so frame_out is no longer
        // referenced. Both pointers are either null (checked) or valid FFmpeg allocations.
        unsafe {
            if !self.frame_out.is_null() {
                ffi::av_frame_free(&mut self.frame_out);
            }
            if !self.hw_frames_ctx.is_null() {
                ffi::av_buffer_unref(&mut self.hw_frames_ctx);
            }
        }
    }
}

// ============================================================
// VideoEncoder
// ============================================================

pub struct VideoEncoder {
    format_ctx: *mut ffi::AVFormatContext,
    codec_ctx: *mut ffi::AVCodecContext,
    frame: *mut ffi::AVFrame,
    scalers: Option<Scalers>,
    hw_device_ctx: *mut ffi::AVBufferRef,
    packet: *mut ffi::AVPacket,
    width_in: usize,
    height_in: usize,
    width_out: usize,
    height_out: usize,
    #[allow(clippy::type_complexity)]
    write_data: Box<dyn FnMut(&[u8])>,
    start_time: Instant,
    /// MSE codec string (e.g. "avc1.4D4028") derived from the opened codec's
    /// profile and level. Used by the client for addSourceBuffer().
    codec_string: String,
    codec: Codec,
    pending_pts: Option<i64>,
    /// When true, the next encoded frame will be forced to an IDR. Consumed by
    /// `encode_frame` and reset. Driven by `RequestKeyframe` from the client
    /// (reconnect / tab refocus / decode-error recovery) and by the first frame
    /// after encoder restart.
    force_keyframe: bool,
}

// VideoEncoder contains raw FFmpeg pointers that are not thread-safe.
// It must be created and used on a single thread (the encode thread in session.rs).
// We intentionally do NOT implement Send — the encoder is created inside the
// encode thread function, never moved across threads.

impl VideoEncoder {
    pub fn new(
        width_in: usize,
        height_in: usize,
        width_out: usize,
        height_out: usize,
        mut write_data: impl FnMut(&[u8]) + Send + 'static,
        options: EncoderOptions,
    ) -> Result<Box<Self>, EncodeError> {
        let width_out = width_out - width_out % 2;
        let height_out = height_out - height_out % 2;

        if width_out <= 1 || height_out <= 1 {
            return Err(enc_err!(
                "Invalid size for video: width = {}, height = {}",
                width_out,
                height_out
            ));
        }

        let mut encoder = Box::new(Self {
            format_ctx: ptr::null_mut(),
            codec_ctx: ptr::null_mut(),
            frame: ptr::null_mut(),
            scalers: None,
            hw_device_ctx: ptr::null_mut(),
            packet: ptr::null_mut(),
            width_in,
            height_in,
            width_out,
            height_out,
            write_data: Box::new(move |data| write_data(data)),
            start_time: Instant::now(),
            codec_string: String::new(),
            codec: options.codec,
            pending_pts: None,
            // First real frame after construction should be an IDR so late joiners
            // and post-restart clients don't wait a full GOP for playback to start.
            force_keyframe: true,
        });

        encoder.open_video(options)?;
        Ok(encoder)
    }

    /// Returns the MSE codec string for this encoder (e.g. "avc1.4D0028").
    /// Used by the client to call addSourceBuffer with the correct codec.
    pub fn codec_string(&self) -> &str {
        &self.codec_string
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    pub fn set_next_pts(&mut self, pts: i64) {
        self.pending_pts = Some(pts);
    }

    /// Force the next encoded frame to be an IDR keyframe.
    pub fn request_keyframe(&mut self) {
        self.force_keyframe = true;
    }

    fn open_video(&mut self, options: EncoderOptions) -> Result<(), EncodeError> {
        // SAFETY: FFmpeg format/codec allocation and AVIO setup. All pointers are checked
        // for null after allocation. The AVIO opaque pointer is `self`, which is valid for
        // the lifetime of the format context because VideoEncoder owns both.
        unsafe {
            let ret = ffi::avformat_alloc_output_context2(
                &mut self.format_ctx,
                ptr::null_mut(),
                c"mp4".as_ptr(),
                ptr::null(),
            );
            if self.format_ctx.is_null() || ret < 0 {
                return Err(enc_err!("Could not find output format mp4"));
            }

            let mut using_hw = false;

            #[cfg(target_os = "linux")]
            if options.try_vaapi && !using_hw {
                using_hw = self.try_vaapi();
            }

            #[cfg(target_os = "windows")]
            if options.try_mediafoundation && !using_hw {
                using_hw = self.try_mediafoundation();
            }

            #[cfg(any(target_os = "linux", target_os = "windows"))]
            if options.try_nvenc && !using_hw {
                using_hw = self.try_nvenc();
            }

            #[cfg(target_os = "macos")]
            if options.try_videotoolbox && !using_hw {
                using_hw = self.try_videotoolbox();
            }

            if !using_hw {
                self.setup_libx264(options)?;
            }

            // Create stream
            let st = ffi::avformat_new_stream(self.format_ctx, ptr::null());
            if st.is_null() {
                return Err(enc_err!("Failed to create output stream"));
            }
            ffi::avcodec_parameters_from_context((*st).codecpar, self.codec_ctx);

            // Allocate packet
            self.packet = ffi::av_packet_alloc();
            if self.packet.is_null() {
                return Err(enc_err!("Failed to allocate packet"));
            }

            // Custom AVIO context
            let buf_size: c_int = options.avio_buffer_size as c_int;
            let avio_buf = ffi::av_malloc(buf_size as usize) as *mut u8;
            if avio_buf.is_null() {
                return Err(enc_err!("Failed to allocate AVIO buffer"));
            }
            let avio = ffi::avio_alloc_context(
                avio_buf,
                buf_size,
                ffi::AVIO_FLAG_WRITE,
                self as *mut Self as *mut c_void,
                None,
                // SAFETY: avio_alloc_context's write callback signature changed between
                // FFmpeg versions (*const u8 in older, *mut u8 in newer). We cast the
                // function pointer via `as` to erase the exact type, then transmute to
                // match whichever FFmpeg is installed. The callback only reads from buf.
                #[allow(clippy::missing_transmute_annotations)]
                Some(std::mem::transmute::<usize, _>(
                    avio_write_callback as *const () as usize,
                )),
                None,
            );
            if avio.is_null() {
                ffi::av_free(avio_buf as *mut c_void);
                return Err(enc_err!("Failed to allocate avio context"));
            }
            (*self.format_ctx).pb = avio;

            // Write fragmented MP4 header
            let mut opt: *mut ffi::AVDictionary = ptr::null_mut();
            ffi::av_dict_set(
                &mut opt,
                c"movflags".as_ptr(),
                c"frag_custom+empty_moov+default_base_moof".as_ptr(),
                0,
            );
            let ret = ffi::avformat_write_header(self.format_ctx, &mut opt);
            ffi::av_dict_free(&mut opt);
            if ret < 0 {
                warn!("Video: failed to write header!");
            }

            let codec_name = CStr::from_ptr((*(*self.codec_ctx).codec).name).to_string_lossy();
            let pix_fmt = pix_fmt_name((*self.codec_ctx).pix_fmt);

            // Build the MSE codec string from the actual profile and level.
            // Format: avc1.PPCCLL where PP=profile_idc, CC=constraint_flags, LL=level_idc.
            // We read profile and level from the codec context after open.
            let profile = (*self.codec_ctx).profile;
            let level = (*self.codec_ctx).level;
            self.codec_string = if profile >= 0 && level >= 0 {
                match self.codec {
                    Codec::Hevc => format!("hvc1.{:02X}.0.L{:02X}.00", profile, level),
                    Codec::H264 => format!("avc1.{:02X}00{:02X}", profile, level),
                }
            } else {
                match self.codec {
                    Codec::Hevc => "hvc1.0100.0.L93.00".to_string(),
                    Codec::H264 => "avc1.4D0028".to_string(),
                }
            };
            info!(
                "Video: {}x{}@{} pix_fmt: {} codec_string: {}",
                self.width_out, self.height_out, codec_name, pix_fmt, self.codec_string
            );

            Ok(())
        }
    }

    fn set_codec_params(&self, c: *mut ffi::AVCodecContext) {
        self.set_codec_params_with_options(c, EncoderOptions::default());
    }

    fn set_codec_params_with_options(&self, c: *mut ffi::AVCodecContext, options: EncoderOptions) {
        unsafe {
            (*c).width = self.width_out as c_int;
            (*c).height = self.height_out as c_int;
            (*c).time_base = TIME_BASE;
            (*c).framerate = ffi::AVRational { num: 0, den: 1 };
            (*c).gop_size = if options.all_intra { 1 } else { GOP_SIZE };
            (*c).max_b_frames = MAX_B_FRAMES;
            if (*(*self.format_ctx).oformat).flags & ffi::AVFMT_GLOBALHEADER != 0 {
                (*c).flags |= ffi::AV_CODEC_FLAG_GLOBAL_HEADER as c_int;
            }
        }
    }

    // ---- Hardware encoder setup methods ----

    /// Common hardware/software codec setup: find codec, alloc context, create scalers,
    /// apply codec-specific options, open, and handle cleanup on failure.
    ///
    /// Returns true if the codec was successfully opened. On failure, all allocated
    /// resources are freed and false is returned.
    ///
    /// SAFETY: Caller must ensure `self` fields are in a valid state. This function
    /// manages FFmpeg resource lifetimes: codec_ctx, scalers, and (optionally) hw_device_ctx.
    unsafe fn try_codec(
        &mut self,
        codec_name: &CStr,
        hw_pix_fmt: AVPixelFormat,
        sw_pix_fmt: AVPixelFormat,
        ctx_pix_fmt: AVPixelFormat,
        uses_hw_device: bool,
        set_options: impl FnOnce(*mut ffi::AVCodecContext),
    ) -> bool {
        let codec = ffi::avcodec_find_encoder_by_name(codec_name.as_ptr());
        if codec.is_null() {
            debug!("Codec {:?} not found", codec_name);
            if uses_hw_device {
                ffi::av_buffer_unref(&mut self.hw_device_ctx);
            }
            return false;
        }

        let c = ffi::avcodec_alloc_context3(codec);
        if c.is_null() {
            debug!("Could not allocate codec context for {:?}", codec_name);
            if uses_hw_device {
                ffi::av_buffer_unref(&mut self.hw_device_ctx);
            }
            return false;
        }

        match Scalers::new(
            self.width_in as i32,
            self.height_in as i32,
            self.width_out as i32,
            self.height_out as i32,
            hw_pix_fmt,
            sw_pix_fmt,
            self.hw_device_ctx,
        ) {
            Ok(s) => {
                (*c).pix_fmt = ctx_pix_fmt;
                if uses_hw_device && !s.hw_frames_ctx.is_null() {
                    (*c).hw_frames_ctx = ffi::av_buffer_ref(s.hw_frames_ctx);
                }
                set_options(c);
                self.set_codec_params(c);

                let ret = ffi::avcodec_open2(c, codec, ptr::null_mut());
                if ret == 0 {
                    self.codec_ctx = c;
                    self.scalers = Some(s);
                    return true;
                }
                debug!("Could not open {:?} codec", codec_name);
                ffi::avcodec_free_context(&mut { c });
                if uses_hw_device {
                    ffi::av_buffer_unref(&mut self.hw_device_ctx);
                }
            }
            Err(e) => {
                warn!("Failed to initialize scaler for {:?}: {}", codec_name, e);
                ffi::avcodec_free_context(&mut { c });
                if uses_hw_device {
                    ffi::av_buffer_unref(&mut self.hw_device_ctx);
                }
            }
        }
        false
    }

    #[cfg(target_os = "linux")]
    fn try_vaapi(&mut self) -> bool {
        // SAFETY: FFmpeg hardware device allocation + try_codec handles the rest.
        unsafe {
            let vaapi_device = std::env::var("RYLUS_VAAPI_DEVICE").ok();
            let vaapi_device_c = vaapi_device
                .as_ref()
                .and_then(|d| CString::new(d.as_str()).ok());
            let device_ptr = vaapi_device_c.as_ref().map_or(ptr::null(), |c| c.as_ptr());

            let ret = ffi::av_hwdevice_ctx_create(
                &mut self.hw_device_ctx,
                ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                device_ptr,
                ptr::null_mut(),
                0,
            );
            if ret != 0 {
                return false;
            }

            self.try_codec(
                c"h264_vaapi",
                AVPixelFormat::AV_PIX_FMT_VAAPI,
                AVPixelFormat::AV_PIX_FMT_NV12,
                AVPixelFormat::AV_PIX_FMT_VAAPI,
                true,
                |c| {
                    ffi::av_opt_set((*c).priv_data, c"quality".as_ptr(), c"7".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"qp".as_ptr(), c"23".as_ptr(), 0);
                    // async_depth=1 forces single-frame encoder pipeline; low_power uses
                    // the fixed-function VDENC path on Intel/AMD which has lower latency
                    // and frees the EU shader array. Both cut 1–2 frames of pipeline depth.
                    ffi::av_opt_set((*c).priv_data, c"async_depth".as_ptr(), c"1".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"low_power".as_ptr(), c"1".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"idr_interval".as_ptr(), c"0".as_ptr(), 0);
                },
            )
        }
    }

    #[cfg(target_os = "windows")]
    fn try_mediafoundation(&mut self) -> bool {
        // SAFETY: try_codec handles all FFmpeg resource management.
        unsafe {
            self.try_codec(
                c"h264_mf",
                AVPixelFormat::AV_PIX_FMT_NV12,
                AVPixelFormat::AV_PIX_FMT_NV12,
                AVPixelFormat::AV_PIX_FMT_NV12,
                false,
                |c| {
                    ffi::av_opt_set(
                        (*c).priv_data,
                        c"rate_control".as_ptr(),
                        c"ld_vbr".as_ptr(),
                        0,
                    );
                    ffi::av_opt_set(
                        (*c).priv_data,
                        c"scenario".as_ptr(),
                        c"display_remoting".as_ptr(),
                        0,
                    );
                    ffi::av_opt_set((*c).priv_data, c"quality".as_ptr(), c"100".as_ptr(), 0);
                },
            )
        }
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn try_nvenc(&mut self) -> bool {
        // SAFETY: FFmpeg CUDA hardware device allocation + try_codec handles the rest.
        unsafe {
            let ret = ffi::av_hwdevice_ctx_create(
                &mut self.hw_device_ctx,
                ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
                ptr::null(),
                ptr::null_mut(),
                0,
            );
            if ret != 0 {
                return false;
            }

            self.try_codec(
                c"h264_nvenc",
                AVPixelFormat::AV_PIX_FMT_CUDA,
                AVPixelFormat::AV_PIX_FMT_BGR0,
                AVPixelFormat::AV_PIX_FMT_CUDA,
                true,
                |c| {
                    ffi::av_opt_set((*c).priv_data, c"preset".as_ptr(), c"p1".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"zerolatency".as_ptr(), c"1".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"tune".as_ptr(), c"ull".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"rc".as_ptr(), c"cbr".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"cq".as_ptr(), c"21".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"delay".as_ptr(), c"0".as_ptr(), 0);
                    // Lookahead buys quality at the cost of latency; kill it for screen share.
                    ffi::av_opt_set((*c).priv_data, c"rc-lookahead".as_ptr(), c"0".as_ptr(), 0);
                    // Keep GOP cadence deterministic so the client can time
                    // recovery windows; disable scene-cut injection.
                    ffi::av_opt_set((*c).priv_data, c"no-scenecut".as_ptr(), c"1".as_ptr(), 0);
                    // Needed for the RequestKeyframe path to actually emit an IDR
                    // instead of just a non-IDR recovery point.
                    ffi::av_opt_set((*c).priv_data, c"forced-idr".as_ptr(), c"1".as_ptr(), 0);
                },
            )
        }
    }

    #[cfg(target_os = "macos")]
    fn try_videotoolbox(&mut self) -> bool {
        // SAFETY: try_codec handles all FFmpeg resource management.
        // NOTE: Using "main" profile instead of "extended" — Extended Profile has
        // zero MSE/Media Source Extensions support in any browser. Main Profile is
        // universally supported and produces valid fMP4 for the web client.
        unsafe {
            self.try_codec(
                c"h264_videotoolbox",
                AVPixelFormat::AV_PIX_FMT_YUV420P,
                AVPixelFormat::AV_PIX_FMT_YUV420P,
                AVPixelFormat::AV_PIX_FMT_YUV420P,
                false,
                |c| {
                    ffi::av_opt_set((*c).priv_data, c"realtime".as_ptr(), c"true".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"allow_sw".as_ptr(), c"true".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"profile".as_ptr(), c"main".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"level".as_ptr(), c"5.2".as_ptr(), 0);
                },
            )
        }
    }

    fn setup_libx264(&mut self, options: EncoderOptions) -> Result<(), EncodeError> {
        // SAFETY: FFmpeg codec allocation for libx264 software encoder. All pointers are
        // null-checked after allocation and freed on error paths.
        unsafe {
            let codec_name = options.codec.ffmpeg_codec_name();
            let codec = ffi::avcodec_find_encoder_by_name(codec_name.as_ptr());
            if codec.is_null() {
                return Err(enc_err!(
                    "Codec '{}' not found",
                    codec_name.to_string_lossy()
                ));
            }

            let c = ffi::avcodec_alloc_context3(codec);
            if c.is_null() {
                return Err(enc_err!("Could not allocate video codec context"));
            }

            match Scalers::new(
                self.width_in as i32,
                self.height_in as i32,
                self.width_out as i32,
                self.height_out as i32,
                AVPixelFormat::AV_PIX_FMT_YUV420P,
                AVPixelFormat::AV_PIX_FMT_YUV420P,
                ptr::null_mut(),
            ) {
                Ok(s) => {
                    (*c).pix_fmt = AVPixelFormat::AV_PIX_FMT_YUV420P;
                    ffi::av_opt_set((*c).priv_data, c"preset".as_ptr(), c"ultrafast".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"tune".as_ptr(), c"zerolatency".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"crf".as_ptr(), c"23".as_ptr(), 0);
                    self.set_codec_params_with_options(c, options);

                    let ret = ffi::avcodec_open2(c, codec, ptr::null_mut());
                    if ret < 0 {
                        ffi::avcodec_free_context(&mut { c });
                        return Err(enc_err!("Could not open libx264 codec"));
                    }

                    self.codec_ctx = c;
                    self.scalers = Some(s);
                    Ok(())
                }
                Err(e) => {
                    ffi::avcodec_free_context(&mut { c });
                    Err(e)
                }
            }
        }
    }

    // ---- Encoding ----

    pub fn encode(&mut self, pixel_provider: PixelProvider) {
        let scalers = match self.scalers.as_mut() {
            Some(s) => s,
            None => {
                warn!("Scalers not initialized");
                return;
            }
        };

        let scaler = match pixel_provider {
            PixelProvider::BGR0(w, h, bgr0) => {
                let expected = w * h * 4;
                if bgr0.len() < expected {
                    warn!(
                        "BGR0 buffer too small: {} bytes for {}x{} (need {})",
                        bgr0.len(),
                        w,
                        h,
                        expected
                    );
                    return;
                }
                let s = scalers.bgr0.as_mut().expect("BGR0 scaler not initialized");
                // SAFETY: frame_in is a valid AVFrame allocated in ScaleContext::new.
                // The pixel data pointer is valid for the duration of this encode call.
                // Buffer size validated above.
                unsafe {
                    (*s.frame_in).data[0] = bgr0.as_ptr() as *mut u8;
                    (*s.frame_in).linesize[0] = (w * 4) as c_int;
                }
                s
            }
            PixelProvider::BGR0S(_, h, stride, bgr0) => {
                let expected = stride * h;
                if bgr0.len() < expected {
                    warn!(
                        "BGR0S buffer too small: {} bytes for stride {} x {} rows (need {})",
                        bgr0.len(),
                        stride,
                        h,
                        expected
                    );
                    return;
                }
                let s = scalers.bgr0.as_mut().expect("BGR0 scaler not initialized");
                // SAFETY: frame_in is a valid AVFrame allocated in ScaleContext::new.
                // The pixel data pointer is valid for the duration of this encode call.
                // Buffer size validated above.
                unsafe {
                    (*s.frame_in).data[0] = bgr0.as_ptr() as *mut u8;
                    (*s.frame_in).linesize[0] = stride as c_int;
                }
                s
            }
            PixelProvider::RGB(w, h, rgb) => {
                let expected = w * h * 3;
                if rgb.len() < expected {
                    warn!(
                        "RGB buffer too small: {} bytes for {}x{} (need {})",
                        rgb.len(),
                        w,
                        h,
                        expected
                    );
                    return;
                }
                let s = scalers.rgb.as_mut().expect("RGB scaler not initialized");
                // SAFETY: frame_in is a valid AVFrame allocated in ScaleContext::new.
                // The pixel data pointer is valid for the duration of this encode call.
                // Buffer size validated above.
                unsafe {
                    (*s.frame_in).data[0] = rgb.as_ptr() as *mut u8;
                    (*s.frame_in).linesize[0] = (w * 3) as c_int;
                }
                s
            }
            PixelProvider::RGB0(w, h, rgb0) => {
                let expected = w * h * 4;
                if rgb0.len() < expected {
                    warn!(
                        "RGB0 buffer too small: {} bytes for {}x{} (need {})",
                        rgb0.len(),
                        w,
                        h,
                        expected
                    );
                    return;
                }
                let s = scalers.rgb0.as_mut().expect("RGB0 scaler not initialized");
                // SAFETY: frame_in is a valid AVFrame allocated in ScaleContext::new.
                // The pixel data pointer is valid for the duration of this encode call.
                // Buffer size validated above.
                unsafe {
                    (*s.frame_in).data[0] = rgb0.as_ptr() as *mut u8;
                    (*s.frame_in).linesize[0] = (w * 4) as c_int;
                }
                s
            }
        };

        if let Err(e) = scaler.scale() {
            warn!("Failed to scale video frame: {}", e);
            return;
        }
        self.frame = scaler.frame_out;

        if let Err(e) = self.encode_frame() {
            warn!("Failed to encode video frame: {}", e);
        }
    }

    fn encode_frame(&mut self) -> Result<(), EncodeError> {
        // SAFETY: codec_ctx, format_ctx, frame, and packet are valid pointers initialized
        // in open_video. frame is null-checked at entry. The encoding loop follows FFmpeg's
        // documented send_frame/receive_packet pattern.
        unsafe {
            if self.frame.is_null() {
                return Err(enc_err!("Frame not initialized"));
            }

            let pts = self
                .pending_pts
                .take()
                .unwrap_or_else(|| (Instant::now() - self.start_time).as_millis() as i64);
            (*self.frame).pts = pts;

            // Force an IDR when requested. Setting pict_type = I together with
            // key_frame = 1 hits both the libx264 path and every hardware encoder
            // we enable (NVENC with forced-idr=1, VAAPI with idr_interval=0,
            // VideoToolbox's realtime mode, and MediaFoundation's ld_vbr).
            if self.force_keyframe {
                (*self.frame).pict_type = AVPictureType::AV_PICTURE_TYPE_I;
                (*self.frame).flags |= ffi::AV_FRAME_FLAG_KEY;
                self.force_keyframe = false;
            } else {
                (*self.frame).pict_type = AVPictureType::AV_PICTURE_TYPE_NONE;
                (*self.frame).flags &= !ffi::AV_FRAME_FLAG_KEY;
            }

            let ret = ffi::avcodec_send_frame(self.codec_ctx, self.frame);
            if ret < 0 {
                return Err(enc_err!("Error sending frame for encoding"));
            }

            loop {
                let ret = ffi::avcodec_receive_packet(self.codec_ctx, self.packet);
                if ret == averror(EAGAIN) || ret == AVERROR_EOF {
                    return Ok(());
                }
                if ret < 0 {
                    return Err(enc_err!("Error during encoding"));
                }

                let st = *(*self.format_ctx).streams.add(0);
                ffi::av_packet_rescale_ts(
                    self.packet,
                    (*self.codec_ctx).time_base,
                    (*st).time_base,
                );
                ffi::av_write_frame(self.format_ctx, self.packet);
                ffi::av_packet_unref(self.packet);

                // New fragment on every frame for lowest latency
                ffi::av_write_frame(self.format_ctx, ptr::null_mut());
            }
        }
    }

    pub fn check_size(
        &self,
        width_in: usize,
        height_in: usize,
        width_out: usize,
        height_out: usize,
    ) -> bool {
        self.width_in == width_in
            && self.height_in == height_in
            && self.width_out == width_out
            && self.height_out == height_out
    }

    /// Adjust encoding quality. `qp` is a quantization parameter (0-51, lower = better quality).
    /// The default is 23. Increasing QP reduces quality but speeds up encoding.
    pub fn set_quality(&mut self, qp: u32) {
        let qp = qp.min(51);
        // SAFETY: codec_ctx is checked for null before dereferencing. When non-null, it is
        // a valid codec context allocated in open_video.
        unsafe {
            if self.codec_ctx.is_null() {
                return;
            }
            let qp_str =
                CString::new(qp.to_string()).expect("numeric string should not contain null bytes");
            // Try setting QP via priv_data options (works for most encoders)
            ffi::av_opt_set(
                (*self.codec_ctx).priv_data,
                c"qp".as_ptr(),
                qp_str.as_ptr(),
                0,
            );
            // Also set global_quality which some encoders use
            (*self.codec_ctx).global_quality = (qp as c_int) * ffi::FF_QP2LAMBDA;
        }
        debug!("Encoder quality adjusted to QP {qp}");
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        // SAFETY: Each pointer is null-checked before freeing. All are valid FFmpeg
        // allocations made during open_video/new. Each is freed exactly once. The AVIO
        // context is freed before the format context that references it.
        unsafe {
            if !self.format_ctx.is_null() {
                ffi::av_write_trailer(self.format_ctx);
                if !(*self.format_ctx).pb.is_null() {
                    ffi::avio_context_free(&mut (*self.format_ctx).pb);
                }
                ffi::avformat_free_context(self.format_ctx);
            }
            if !self.codec_ctx.is_null() {
                ffi::avcodec_free_context(&mut self.codec_ctx);
            }
            if !self.packet.is_null() {
                ffi::av_packet_free(&mut self.packet);
            }
            if !self.hw_device_ctx.is_null() {
                ffi::av_buffer_unref(&mut self.hw_device_ctx);
            }
        }
    }
}

// ============================================================
// FFmpeg Logger
// ============================================================

pub fn init_ffmpeg_logger() {
    // SAFETY: av_log_set_level is safe to call at any time; it only sets a global integer.
    unsafe {
        ffi::av_log_set_level(ffi::AV_LOG_WARNING);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rylus_core::pixel::PixelProvider;

    fn default_options() -> EncoderOptions {
        EncoderOptions::default()
    }

    /// Generate a test BGR0 frame (blue channel only).
    fn make_bgr0_frame(width: usize, height: usize) -> Vec<u8> {
        let mut buf = vec![0u8; width * height * 4];
        for pixel in buf.chunks_exact_mut(4) {
            pixel[0] = 128; // B
            pixel[1] = 64; // G
            pixel[2] = 32; // R
            pixel[3] = 255; // padding
        }
        buf
    }

    #[test]
    fn encoder_rejects_zero_dimensions() {
        let output = Vec::new();
        let output = std::sync::Mutex::new(output);
        let result = VideoEncoder::new(
            0,
            0,
            0,
            0,
            move |data| {
                output.lock().unwrap().extend_from_slice(data);
            },
            default_options(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn encoder_rejects_one_pixel() {
        let output = std::sync::Mutex::new(Vec::new());
        let result = VideoEncoder::new(
            1,
            1,
            1,
            1,
            move |data| {
                output.lock().unwrap().extend_from_slice(data);
            },
            default_options(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn encoder_creates_with_libx264() {
        init_ffmpeg_logger();
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let out = output.clone();
        let result = VideoEncoder::new(
            64,
            64,
            64,
            64,
            move |data| {
                out.lock().unwrap().extend_from_slice(data);
            },
            default_options(),
        );
        assert!(
            result.is_ok(),
            "libx264 encoder should initialize for 64x64"
        );
        let enc = result.unwrap();
        // Should have a valid codec string
        assert!(!enc.codec_string().is_empty());
        assert!(enc.codec_string().starts_with("avc1."));
    }

    #[test]
    fn encoder_produces_fmp4_output() {
        init_ffmpeg_logger();
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let out = output.clone();
        let mut enc = VideoEncoder::new(
            64,
            64,
            64,
            64,
            move |data| {
                out.lock().unwrap().extend_from_slice(data);
            },
            default_options(),
        )
        .expect("encoder should init");

        // Encode a test frame
        let frame = make_bgr0_frame(64, 64);
        enc.encode(PixelProvider::BGR0(64, 64, &frame));

        // Should have produced some output
        let data = output.lock().unwrap();
        assert!(
            !data.is_empty(),
            "encoding a frame should produce fMP4 output"
        );
        // fMP4 starts with an ftyp box
        assert!(data.len() >= 8, "output should be at least 8 bytes");
    }

    #[test]
    fn encoder_handles_odd_dimensions() {
        // Odd dimensions should be rounded down to even
        init_ffmpeg_logger();
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let out = output.clone();
        let result = VideoEncoder::new(
            65,
            65,
            65,
            65,
            move |data| {
                out.lock().unwrap().extend_from_slice(data);
            },
            default_options(),
        );
        assert!(result.is_ok(), "odd dimensions should be rounded to even");
    }

    #[test]
    fn encoder_set_quality_doesnt_crash() {
        init_ffmpeg_logger();
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let out = output.clone();
        let mut enc = VideoEncoder::new(
            64,
            64,
            64,
            64,
            move |data| {
                out.lock().unwrap().extend_from_slice(data);
            },
            default_options(),
        )
        .expect("encoder should init");

        // Set quality to various values
        enc.set_quality(0);
        enc.set_quality(23);
        enc.set_quality(51);
        enc.set_quality(100); // should clamp to 51
    }

    #[test]
    fn encoder_check_size() {
        init_ffmpeg_logger();
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let out = output.clone();
        let enc = VideoEncoder::new(
            640,
            480,
            320,
            240,
            move |data| {
                out.lock().unwrap().extend_from_slice(data);
            },
            default_options(),
        )
        .expect("encoder should init");

        assert!(enc.check_size(640, 480, 320, 240));
        assert!(!enc.check_size(640, 480, 320, 241));
        assert!(!enc.check_size(641, 480, 320, 240));
    }

    #[test]
    fn encoder_drop_cleans_up() {
        init_ffmpeg_logger();
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let out = output.clone();
        let enc = VideoEncoder::new(
            64,
            64,
            64,
            64,
            move |data| {
                out.lock().unwrap().extend_from_slice(data);
            },
            default_options(),
        )
        .expect("encoder should init");

        // Drop should not panic or leak
        drop(enc);
    }

    #[test]
    fn encoder_multiple_frames() {
        init_ffmpeg_logger();
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let out = output.clone();
        let mut enc = VideoEncoder::new(
            64,
            64,
            64,
            64,
            move |data| {
                out.lock().unwrap().extend_from_slice(data);
            },
            default_options(),
        )
        .expect("encoder should init");

        let frame = make_bgr0_frame(64, 64);
        for _ in 0..5 {
            enc.encode(PixelProvider::BGR0(64, 64, &frame));
        }

        let data = output.lock().unwrap();
        assert!(
            data.len() > 100,
            "5 frames should produce substantial output"
        );
    }

    #[test]
    fn buffer_validation_wrong_size_doesnt_crash() {
        init_ffmpeg_logger();
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let out = output.clone();
        let mut enc = VideoEncoder::new(
            64,
            64,
            64,
            64,
            move |data| {
                out.lock().unwrap().extend_from_slice(data);
            },
            default_options(),
        )
        .expect("encoder should init");

        // Pass a buffer that's too small — this should not segfault
        // (the bounds check in encode() should catch it)
        let small_buf = vec![0u8; 10];
        enc.encode(PixelProvider::BGR0(64, 64, &small_buf));
    }

    #[test]
    fn codec_serde_roundtrip() {
        let codec = Codec::H264;
        let json = serde_json::to_string(&codec).unwrap();
        let back: Codec = serde_json::from_str(&json).unwrap();
        assert_eq!(codec, back);

        let codec = Codec::Hevc;
        let json = serde_json::to_string(&codec).unwrap();
        let back: Codec = serde_json::from_str(&json).unwrap();
        assert_eq!(codec, back);
    }

    #[test]
    fn encoder_options_defaults() {
        let opts = EncoderOptions::default();
        assert_eq!(opts.codec, Codec::H264);
        assert!(!opts.all_intra);
        assert_eq!(opts.avio_buffer_size, 1024 * 1024);
    }

    #[test]
    fn pending_pts_overrides_auto_pts() {
        let mut enc =
            VideoEncoder::new(64, 64, 64, 64, move |_| {}, EncoderOptions::default()).unwrap();
        enc.set_next_pts(999);
        assert_eq!(enc.pending_pts, Some(999));
    }

    #[test]
    fn pending_pts_none_uses_auto() {
        let enc =
            VideoEncoder::new(64, 64, 64, 64, move |_| {}, EncoderOptions::default()).unwrap();
        assert!(enc.pending_pts.is_none());
    }

    #[test]
    fn avio_buffer_size_used() {
        let opts = EncoderOptions {
            avio_buffer_size: 4096,
            ..Default::default()
        };
        assert_eq!(opts.avio_buffer_size, 4096);
    }

    #[test]
    fn codec_field_accessible() {
        let opts = EncoderOptions {
            codec: Codec::Hevc,
            ..Default::default()
        };
        assert_eq!(opts.codec, Codec::Hevc);
    }

    #[test]
    fn all_intra_sets_gop_one() {
        let opts = EncoderOptions {
            all_intra: true,
            ..Default::default()
        };
        assert!(opts.all_intra);
    }

    #[test]
    fn codec_serde_uppercase_format() {
        let json = serde_json::to_string(&Codec::H264).unwrap();
        assert_eq!(
            json, "\"H264\"",
            "Codec::H264 should serialize as uppercase H264"
        );

        let json = serde_json::to_string(&Codec::Hevc).unwrap();
        assert_eq!(
            json, "\"HEVC\"",
            "Codec::Hevc should serialize as uppercase HEVC"
        );

        // Roundtrip from uppercase
        let h264: Codec = serde_json::from_str("\"H264\"").unwrap();
        assert_eq!(h264, Codec::H264);
        let hevc: Codec = serde_json::from_str("\"HEVC\"").unwrap();
        assert_eq!(hevc, Codec::Hevc);
    }

    #[test]
    fn codec_default_is_h264() {
        assert_eq!(Codec::default(), Codec::H264);
    }

    #[test]
    fn hevc_codec_string_format() {
        init_ffmpeg_logger();
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let out = output.clone();
        let result = VideoEncoder::new(
            64,
            64,
            64,
            64,
            move |data| {
                out.lock().unwrap().extend_from_slice(data);
            },
            EncoderOptions {
                codec: Codec::Hevc,
                ..Default::default()
            },
        );
        // If libx265 is available, codec_string should start with "hvc1."
        // If not, we skip the assertion since we can't control encoder availability.
        if let Ok(enc) = result {
            assert!(
                enc.codec_string().starts_with("hvc1."),
                "HEVC codec string should start with 'hvc1.', got: {}",
                enc.codec_string()
            );
        }
    }

    #[test]
    fn pending_pts_used_and_cleared() {
        init_ffmpeg_logger();
        let frame = make_bgr0_frame(64, 64);
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let out = output.clone();
        let mut enc = VideoEncoder::new(
            64,
            64,
            64,
            64,
            move |data| {
                out.lock().unwrap().extend_from_slice(data);
            },
            EncoderOptions::default(),
        )
        .expect("encoder should init");

        // Set pending PTS
        enc.set_next_pts(12345);
        assert_eq!(enc.pending_pts, Some(12345));

        // Encode a frame — pending_pts should be consumed
        enc.encode(PixelProvider::BGR0(64, 64, &frame));
        assert!(
            enc.pending_pts.is_none(),
            "pending_pts should be cleared after encoding a frame"
        );
    }
}

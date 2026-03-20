use std::ffi::{CStr, CString};
use std::os::raw::{c_int, c_void};
use std::ptr;
use std::time::Instant;

use tracing::{debug, info, warn};

use rylus_core::pixel::PixelProvider;

use ffi::AVPixelFormat;
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

#[derive(Clone, Copy)]
pub struct EncoderOptions {
    pub try_vaapi: bool,
    pub try_nvenc: bool,
    pub try_videotoolbox: bool,
    pub try_mediafoundation: bool,
}

// ============================================================
// Constants
// ============================================================

const TIME_BASE: ffi::AVRational = ffi::AVRational { num: 1, den: 1000 };

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

            // Set output pixel format on buffersink using av_opt_set_bin
            // (equivalent to the C macro av_opt_set_int_list for a single format)
            let pix_fmts: [c_int; 1] = [pix_fmt_out as c_int];
            let ret = ffi::av_opt_set_bin(
                buffersink_ctx as *mut c_void,
                c"pix_fmts".as_ptr(),
                pix_fmts.as_ptr() as *const u8,
                std::mem::size_of_val(&pix_fmts) as c_int,
                ffi::AV_OPT_SEARCH_CHILDREN,
            );
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
                (*frames_ctx).initial_pool_size = 20;
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
}

// SAFETY: VideoEncoder is used from a single thread (the video thread in session.rs).
unsafe impl Send for VideoEncoder {}

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
        });

        encoder.open_video(options)?;
        Ok(encoder)
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
                self.setup_libx264()?;
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
            let buf_size: c_int = 1024 * 1024;
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
                Some(avio_write_callback),
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
            info!(
                "Video: {}x{}@{} pix_fmt: {}",
                self.width_out, self.height_out, codec_name, pix_fmt
            );

            Ok(())
        }
    }

    fn set_codec_params(&self, c: *mut ffi::AVCodecContext) {
        // SAFETY: `c` is a freshly allocated codec context from avcodec_alloc_context3 and
        // format_ctx was allocated in open_video; both are valid, non-null pointers.
        unsafe {
            (*c).width = self.width_out as c_int;
            (*c).height = self.height_out as c_int;
            (*c).time_base = TIME_BASE;
            (*c).framerate = ffi::AVRational { num: 0, den: 1 };
            (*c).gop_size = 12;
            (*c).max_b_frames = 0;
            if (*(*self.format_ctx).oformat).flags & ffi::AVFMT_GLOBALHEADER != 0 {
                (*c).flags |= ffi::AV_CODEC_FLAG_GLOBAL_HEADER as c_int;
            }
        }
    }

    // ---- Hardware encoder setup methods ----

    #[cfg(target_os = "linux")]
    fn try_vaapi(&mut self) -> bool {
        // SAFETY: FFmpeg hardware device and codec allocation. All pointers are null-checked
        // after allocation and freed on error paths. hw_device_ctx is stored in self only on
        // success.
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

            let codec = ffi::avcodec_find_encoder_by_name(c"h264_vaapi".as_ptr());
            if codec.is_null() {
                ffi::av_buffer_unref(&mut self.hw_device_ctx);
                return false;
            }

            let c = ffi::avcodec_alloc_context3(codec);
            if c.is_null() {
                ffi::av_buffer_unref(&mut self.hw_device_ctx);
                return false;
            }

            match Scalers::new(
                self.width_in as i32,
                self.height_in as i32,
                self.width_out as i32,
                self.height_out as i32,
                AVPixelFormat::AV_PIX_FMT_VAAPI,
                AVPixelFormat::AV_PIX_FMT_NV12,
                self.hw_device_ctx,
            ) {
                Ok(s) => {
                    (*c).pix_fmt = AVPixelFormat::AV_PIX_FMT_VAAPI;
                    (*c).hw_frames_ctx = ffi::av_buffer_ref(s.hw_frames_ctx);
                    ffi::av_opt_set((*c).priv_data, c"quality".as_ptr(), c"7".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"qp".as_ptr(), c"23".as_ptr(), 0);
                    self.set_codec_params(c);

                    let ret = ffi::avcodec_open2(c, codec, ptr::null_mut());
                    if ret == 0 {
                        self.codec_ctx = c;
                        self.scalers = Some(s);
                        return true;
                    }
                    debug!("Could not open VAAPI codec");
                    ffi::avcodec_free_context(&mut { c });
                    ffi::av_buffer_unref(&mut self.hw_device_ctx);
                }
                Err(e) => {
                    warn!("Failed to initialize VAAPI scaler: {}", e);
                    ffi::avcodec_free_context(&mut { c });
                    ffi::av_buffer_unref(&mut self.hw_device_ctx);
                }
            }
            false
        }
    }

    #[cfg(target_os = "windows")]
    fn try_mediafoundation(&mut self) -> bool {
        // SAFETY: FFmpeg codec allocation for MediaFoundation encoder. All pointers are
        // null-checked after allocation and freed on error paths.
        unsafe {
            let codec = ffi::avcodec_find_encoder_by_name(c"h264_mf".as_ptr());
            if codec.is_null() {
                debug!("Codec 'h264_mf' not found!");
                return false;
            }

            let c = ffi::avcodec_alloc_context3(codec);
            if c.is_null() {
                debug!("Could not allocate video codec context for 'h264_mf'!");
                return false;
            }

            match Scalers::new(
                self.width_in as i32,
                self.height_in as i32,
                self.width_out as i32,
                self.height_out as i32,
                AVPixelFormat::AV_PIX_FMT_NV12,
                AVPixelFormat::AV_PIX_FMT_NV12,
                ptr::null_mut(),
            ) {
                Ok(s) => {
                    (*c).pix_fmt = AVPixelFormat::AV_PIX_FMT_NV12;
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
                    self.set_codec_params(c);

                    let ret = ffi::avcodec_open2(c, codec, ptr::null_mut());
                    if ret == 0 {
                        self.codec_ctx = c;
                        self.scalers = Some(s);
                        return true;
                    }
                    debug!("Could not open h264_mf codec");
                    ffi::avcodec_free_context(&mut { c });
                }
                Err(e) => {
                    warn!("Failed to initialize MediaFoundation scaler: {}", e);
                    ffi::avcodec_free_context(&mut { c });
                }
            }
            false
        }
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn try_nvenc(&mut self) -> bool {
        // SAFETY: FFmpeg CUDA hardware device and NVENC codec allocation. All pointers are
        // null-checked after allocation and freed on error paths.
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

            let codec = ffi::avcodec_find_encoder_by_name(c"h264_nvenc".as_ptr());
            if codec.is_null() {
                debug!("Codec 'h264_nvenc' not found!");
                ffi::av_buffer_unref(&mut self.hw_device_ctx);
                return false;
            }

            let c = ffi::avcodec_alloc_context3(codec);
            if c.is_null() {
                debug!("Could not allocate video codec context for 'h264_nvenc'!");
                ffi::av_buffer_unref(&mut self.hw_device_ctx);
                return false;
            }

            match Scalers::new(
                self.width_in as i32,
                self.height_in as i32,
                self.width_out as i32,
                self.height_out as i32,
                AVPixelFormat::AV_PIX_FMT_CUDA,
                AVPixelFormat::AV_PIX_FMT_BGR0,
                self.hw_device_ctx,
            ) {
                Ok(s) => {
                    (*c).pix_fmt = AVPixelFormat::AV_PIX_FMT_CUDA;
                    (*c).hw_frames_ctx = ffi::av_buffer_ref(s.hw_frames_ctx);
                    ffi::av_opt_set((*c).priv_data, c"preset".as_ptr(), c"p1".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"zerolatency".as_ptr(), c"1".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"tune".as_ptr(), c"ull".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"rc".as_ptr(), c"cbr".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"cq".as_ptr(), c"21".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"delay".as_ptr(), c"0".as_ptr(), 0);
                    self.set_codec_params(c);

                    let ret = ffi::avcodec_open2(c, codec, ptr::null_mut());
                    if ret == 0 {
                        self.codec_ctx = c;
                        self.scalers = Some(s);
                        return true;
                    }
                    debug!("Could not open h264_nvenc codec");
                    ffi::avcodec_free_context(&mut { c });
                    ffi::av_buffer_unref(&mut self.hw_device_ctx);
                }
                Err(e) => {
                    warn!("Failed to initialize NVENC scaler: {}", e);
                    ffi::avcodec_free_context(&mut { c });
                    ffi::av_buffer_unref(&mut self.hw_device_ctx);
                }
            }
            false
        }
    }

    #[cfg(target_os = "macos")]
    fn try_videotoolbox(&mut self) -> bool {
        // SAFETY: FFmpeg codec allocation for VideoToolbox encoder. All pointers are
        // null-checked after allocation and freed on error paths.
        unsafe {
            let codec = ffi::avcodec_find_encoder_by_name(c"h264_videotoolbox".as_ptr());
            if codec.is_null() {
                return false;
            }

            let c = ffi::avcodec_alloc_context3(codec);
            if c.is_null() {
                return false;
            }

            match Scalers::new(
                self.width_in as i32,
                self.height_in as i32,
                self.width_out as i32,
                self.height_out as i32,
                AVPixelFormat::AV_PIX_FMT_YUV420P,
                AVPixelFormat::AV_PIX_FMT_YUV420P,
                self.hw_device_ctx,
            ) {
                Ok(s) => {
                    (*c).pix_fmt = AVPixelFormat::AV_PIX_FMT_YUV420P;
                    ffi::av_opt_set((*c).priv_data, c"realtime".as_ptr(), c"true".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"allow_sw".as_ptr(), c"true".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"profile".as_ptr(), c"extended".as_ptr(), 0);
                    ffi::av_opt_set((*c).priv_data, c"level".as_ptr(), c"5.2".as_ptr(), 0);
                    self.set_codec_params(c);

                    let ret = ffi::avcodec_open2(c, codec, ptr::null_mut());
                    if ret == 0 {
                        self.codec_ctx = c;
                        self.scalers = Some(s);
                        return true;
                    }
                    debug!("Could not open h264_videotoolbox codec");
                    ffi::avcodec_free_context(&mut { c });
                }
                Err(e) => {
                    warn!("Failed to initialize VideoToolbox scaler: {}", e);
                    ffi::avcodec_free_context(&mut { c });
                }
            }
            false
        }
    }

    fn setup_libx264(&mut self) -> Result<(), EncodeError> {
        // SAFETY: FFmpeg codec allocation for libx264 software encoder. All pointers are
        // null-checked after allocation and freed on error paths.
        unsafe {
            let codec = ffi::avcodec_find_encoder_by_name(c"libx264".as_ptr());
            if codec.is_null() {
                return Err(enc_err!("Codec 'libx264' not found"));
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
                    self.set_codec_params(c);

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
            PixelProvider::BGR0(w, _, bgr0) => {
                let s = scalers.bgr0.as_mut().expect("BGR0 scaler not initialized");
                // SAFETY: frame_in is a valid AVFrame allocated in ScaleContext::new.
                // The pixel data pointer is valid for the duration of this encode call.
                unsafe {
                    (*s.frame_in).data[0] = bgr0.as_ptr() as *mut u8;
                    (*s.frame_in).linesize[0] = (w * 4) as c_int;
                }
                s
            }
            PixelProvider::BGR0S(_, _, stride, bgr0) => {
                let s = scalers.bgr0.as_mut().expect("BGR0 scaler not initialized");
                // SAFETY: frame_in is a valid AVFrame allocated in ScaleContext::new.
                // The pixel data pointer is valid for the duration of this encode call.
                unsafe {
                    (*s.frame_in).data[0] = bgr0.as_ptr() as *mut u8;
                    (*s.frame_in).linesize[0] = stride as c_int;
                }
                s
            }
            PixelProvider::RGB(w, _, rgb) => {
                let s = scalers.rgb.as_mut().expect("RGB scaler not initialized");
                // SAFETY: frame_in is a valid AVFrame allocated in ScaleContext::new.
                // The pixel data pointer is valid for the duration of this encode call.
                unsafe {
                    (*s.frame_in).data[0] = rgb.as_ptr() as *mut u8;
                    (*s.frame_in).linesize[0] = (w * 3) as c_int;
                }
                s
            }
            PixelProvider::RGB0(w, _, rgb0) => {
                let s = scalers.rgb0.as_mut().expect("RGB0 scaler not initialized");
                // SAFETY: frame_in is a valid AVFrame allocated in ScaleContext::new.
                // The pixel data pointer is valid for the duration of this encode call.
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

            let millis = (Instant::now() - self.start_time).as_millis();
            (*self.frame).pts = millis as i64;

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

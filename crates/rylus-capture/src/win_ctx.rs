use std::mem::zeroed;
use std::{mem, ptr};
use winapi::shared::dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput, IID_IDXGIFactory1,
    DXGI_OUTPUT_DESC,
};

use winapi::shared::windef::*;
use winapi::shared::winerror::*;
use winapi::um::winuser::*;
use wio::com::ComPtr;

// from https://github.com/bryal/dxgcap-rs/blob/009b746d1c19c4c10921dd469eaee483db6aa002/src/lib.r
fn hr_failed(hr: HRESULT) -> bool {
    hr < 0
}

fn create_dxgi_factory_1() -> Result<ComPtr<IDXGIFactory1>, String> {
    // SAFETY: CreateDXGIFactory1 is called with the correct IID and a valid output pointer.
    // The HRESULT is checked before wrapping the raw pointer in a ComPtr.
    unsafe {
        let mut factory = ptr::null_mut();
        let hr = CreateDXGIFactory1(&IID_IDXGIFactory1, &mut factory);
        if hr_failed(hr) {
            Err(format!("Failed to create DXGIFactory1, {:x}", hr))
        } else {
            Ok(ComPtr::from_raw(factory as *mut IDXGIFactory1))
        }
    }
}

fn get_adapter_outputs(adapter: &IDXGIAdapter1) -> Vec<ComPtr<IDXGIOutput>> {
    let mut outputs = Vec::new();
    for i in 0.. {
        // SAFETY: adapter is a valid IDXGIAdapter1 from EnumAdapters1. EnumOutputs returns
        // DXGI_ERROR_NOT_FOUND when the index exceeds available outputs, which we use to
        // break. GetDesc populates a valid DXGI_OUTPUT_DESC for attached outputs.
        unsafe {
            let mut output = ptr::null_mut();
            if hr_failed(adapter.EnumOutputs(i, &mut output)) {
                break;
            } else {
                let mut out_desc = zeroed();
                (*output).GetDesc(&mut out_desc);
                if out_desc.AttachedToDesktop != 0 {
                    outputs.push(ComPtr::from_raw(output))
                } else {
                    break;
                }
            }
        }
    }
    outputs
}

#[derive(Clone)]
pub struct WinCtx {
    outputs: Vec<DXGI_OUTPUT_DESC>,
    union_rect: RECT,
}

impl WinCtx {
    pub fn new() -> WinCtx {
        let mut desktops: Vec<DXGI_OUTPUT_DESC> = Vec::new();
        let mut union: RECT;
        // SAFETY: RECT and DXGI_OUTPUT_DESC are C-repr structs where zeroed memory is valid.
        // The factory and adapter are obtained from DXGI APIs with HRESULT checks. UnionRect
        // receives valid pointers to stack-allocated RECTs.
        unsafe {
            union = mem::zeroed();
            let factory = match create_dxgi_factory_1() {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!("{}", e);
                    return WinCtx {
                        outputs: desktops,
                        union_rect: union,
                    };
                }
            };
            let mut adapter = ptr::null_mut();
            if factory.EnumAdapters1(0, &mut adapter) != DXGI_ERROR_NOT_FOUND {
                let adp = ComPtr::from_raw(adapter);
                let outputs = get_adapter_outputs(&adp);
                for o in outputs {
                    let mut desc: DXGI_OUTPUT_DESC = mem::zeroed();
                    o.GetDesc(ptr::addr_of_mut!(desc));
                    desktops.push(desc);
                    UnionRect(
                        ptr::addr_of_mut!(union),
                        ptr::addr_of!(union),
                        ptr::addr_of!(desc.DesktopCoordinates),
                    );
                }
            }
        }
        WinCtx {
            outputs: desktops,
            union_rect: union,
        }
    }
    pub fn get_outputs(&self) -> &Vec<DXGI_OUTPUT_DESC> {
        &self.outputs
    }
    pub fn get_union_rect(&self) -> &RECT {
        &self.union_rect
    }
}

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput, DXGI_OUTPUT_DESC,
};
use windows::Win32::Graphics::Gdi::UnionRect;

fn create_dxgi_factory_1() -> Result<IDXGIFactory1, String> {
    // SAFETY: CreateDXGIFactory1 is a safe wrapper in the `windows` crate that returns Result.
    unsafe {
        CreateDXGIFactory1::<IDXGIFactory1>()
            .map_err(|e| format!("Failed to create DXGIFactory1: {}", e))
    }
}

fn get_adapter_outputs(adapter: &IDXGIAdapter1) -> Vec<IDXGIOutput> {
    let mut outputs = Vec::new();
    for i in 0.. {
        // SAFETY: EnumOutputs returns an error when the index exceeds available outputs.
        // GetDesc populates a valid DXGI_OUTPUT_DESC for attached outputs.
        unsafe {
            match adapter.EnumOutputs(i) {
                Ok(output) => {
                    match output.GetDesc() {
                        Ok(desc) => {
                            if desc.AttachedToDesktop == true {
                                outputs.push(output);
                            } else {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                Err(_) => break,
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
        let mut union = RECT::default();

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

        // SAFETY: EnumAdapters1 returns an error when the index exceeds available adapters.
        // GetDesc and UnionRect are called with valid pointers to stack-allocated structs.
        unsafe {
            if let Ok(adapter) = factory.EnumAdapters1(0) {
                let outputs = get_adapter_outputs(&adapter);
                for o in outputs {
                    if let Ok(desc) = o.GetDesc() {
                        desktops.push(desc);
                        let prev = union;
                        let _ = UnionRect(&mut union, &prev, &desc.DesktopCoordinates);
                    }
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

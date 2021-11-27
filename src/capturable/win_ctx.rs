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

fn create_dxgi_factory_1() -> ComPtr<IDXGIFactory1> {
    unsafe {
        let mut factory = ptr::null_mut();
        let hr = CreateDXGIFactory1(&IID_IDXGIFactory1, &mut factory);
        if hr_failed(hr) {
            panic!("Failed to create DXGIFactory1, {:x}", hr)
        } else {
            ComPtr::from_raw(factory as *mut IDXGIFactory1)
        }
    }
}

fn get_adapter_outputs(adapter: &IDXGIAdapter1) -> Vec<ComPtr<IDXGIOutput>> {
    let mut outputs = Vec::new();
    for i in 0.. {
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
    outputs: Vec<RECT>,
    union_rect: RECT,
}

impl WinCtx {
    pub fn new() -> WinCtx {
        let mut rects: Vec<RECT> = Vec::new();
        let mut union: RECT;
        unsafe {
            union = mem::zeroed();
            let factory = create_dxgi_factory_1();
            let mut adapter = ptr::null_mut();
            if factory.EnumAdapters1(0, &mut adapter) != DXGI_ERROR_NOT_FOUND {
                let adp = ComPtr::from_raw(adapter);
                let outputs = get_adapter_outputs(&adp);
                for o in outputs {
                    let mut desc: DXGI_OUTPUT_DESC = mem::zeroed();
                    o.GetDesc(ptr::addr_of_mut!(desc));
                    rects.push(desc.DesktopCoordinates);
                    UnionRect(
                        ptr::addr_of_mut!(union),
                        ptr::addr_of!(union),
                        ptr::addr_of!(desc.DesktopCoordinates),
                    );
                }
            }
        }
        WinCtx {
            outputs: rects,
            union_rect: union,
        }
    }
    pub fn get_outputs(&self) -> &Vec<RECT> {
        &self.outputs
    }
    pub fn get_union_rect(&self) -> &RECT {
        &self.union_rect
    }
}

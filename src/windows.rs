use crate::Error;
use async_trait::async_trait;
use futures::Stream;
use maplit::hashmap;
use std::collections::HashMap;
use std::ffi::{c_void, OsString};
use std::iter;
use std::os::windows::ffi::OsStringExt;

use brightness_windows::windows::{self, HRESULT};
use brightness_windows::Windows::Win32::{
    Devices::Display::{
        DestroyPhysicalMonitor, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, DISPLAYPOLICY_AC, DISPLAYPOLICY_DC,
        DISPLAY_BRIGHTNESS, IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS,
        IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS, IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS,
        PHYSICAL_MONITOR,
    },
    Foundation::{CloseHandle, BOOL, HANDLE, LPARAM, PWSTR, RECT},
    Graphics::Gdi::{
        EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW, HDC, HMONITOR,
        MONITORINFO, MONITORINFOEXW,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_ACCESS_FLAGS, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    },
    System::Diagnostics::Debug::{ERROR_ACCESS_DENIED, ERROR_NOT_FOUND},
    System::SystemServices::{DeviceIoControl, GENERIC_READ, GENERIC_WRITE},
    UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME,
};

#[derive(Debug)]
pub struct Brightness {
    physical_monitor: WrappedPhysicalMonitor,
    file_handle: WrappedFileHandle,
    device_name: String,
    device_description: String,
    device_id: String,
    device_key: String,
    device_string: String,
}

#[derive(Debug)]
struct WrappedPhysicalMonitor(HANDLE);

impl Drop for WrappedPhysicalMonitor {
    fn drop(&mut self) {
        unsafe {
            DestroyPhysicalMonitor(self.0);
        }
    }
}

#[derive(Debug)]
struct WrappedFileHandle(HANDLE);

impl Drop for WrappedFileHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[async_trait]
impl crate::Brightness for Brightness {
    async fn device_name(&self) -> Result<String, Error> {
        Ok(self.device_name.clone())
    }

    async fn device_info(&self) -> Result<HashMap<String, String>, Error> {
        Ok(hashmap! {
            "device_description".to_string() => self.device_description.clone(),
            "device_id".to_string() => self.device_id.clone(),
            "device_key".to_string() => self.device_key.clone(),
            "device_string".to_string() => self.device_string.clone(),
        })
    }

    async fn get(&self) -> Result<u32, Error> {
        let ioctl_query = ioctl_query_supported_brightness(self);
        match ioctl_query {
            Ok(_) => ioctl_query_display_brightness(self).map_err(|e| e.into()),
            Err(e) if e.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => {
                ddcci_get_monitor_brightness(self)
                    .map(|b| b.get_current_percentage())
                    .map_err(|e| e.into())
            }
            Err(e) => Err(SysError::IoctlQuerySupportedBrightnessFailed {
                device_name: self.device_name.clone(),
                source: e,
            }
            .into()),
        }
    }

    async fn set(&mut self, percentage: u32) -> Result<(), Error> {
        let ioctl_query = ioctl_query_supported_brightness(self);
        match ioctl_query {
            Ok(levels) => {
                let new_value = levels.get_nearest(percentage);
                ioctl_set_display_brightness(self, new_value).map_err(|e| e.into())
            }
            Err(e) if e.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => {
                let new_value = ddcci_get_monitor_brightness(self)
                    .map(|b| b.percentage_to_current(percentage))?;
                ddcci_set_monitor_brightness(self, new_value).map_err(|e| e.into())
            }
            Err(e) => Err(SysError::IoctlQuerySupportedBrightnessFailed {
                device_name: self.device_name.clone(),
                source: e,
            }
            .into()),
        }
    }
}

pub fn brightness_devices() -> impl Stream<Item = Result<Brightness, SysError>> {
    unsafe extern "system" fn enum_monitors(
        handle: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(data.0 as *mut Vec<HMONITOR>);
        monitors.push(handle);
        return true.into();
    }
    let mut hmonitors = Vec::<HMONITOR>::new();
    let devices = unsafe {
        match EnumDisplayMonitors(
            HDC::NULL,
            std::ptr::null_mut(),
            Some(enum_monitors),
            LPARAM((&mut hmonitors as *mut _) as isize),
        )
        .ok()
        {
            Err(e) => either::Left(iter::once(Err(SysError::EnumDeviceMonitorsFailed(e)))),
            Ok(_) => {
                either::Right(hmonitors.into_iter().flat_map(|hmonitor| {
                    // Get the Name of the HMONITOR
                    let mut info = MONITORINFOEXW::default();
                    info.__AnonymousBase_winuser_L13558_C43.cbSize =
                        std::mem::size_of::<MONITORINFOEXW>() as u32;
                    let info_ptr = &mut info as *mut _;
                    if let Err(e) = GetMonitorInfoW(hmonitor, info_ptr as *mut MONITORINFO).ok() {
                        return vec![Err(SysError::GetMonitorInfoFailed(e))];
                    };

                    // Get the Physical Monitors in the HMONITOR
                    let mut physical_number: u32 = 0;
                    if let Err(e) = BOOL(GetNumberOfPhysicalMonitorsFromHMONITOR(
                        hmonitor,
                        &mut physical_number,
                    ))
                    .ok()
                    {
                        return vec![Err(SysError::GetPhysicalMonitorsFailed(e))];
                    };
                    let mut physical_monitors = {
                        let monitor = PHYSICAL_MONITOR {
                            hPhysicalMonitor: HANDLE::NULL,
                            szPhysicalMonitorDescription: [0; 128],
                        };
                        vec![monitor; physical_number as usize]
                    };
                    if let Err(e) = BOOL(GetPhysicalMonitorsFromHMONITOR(
                        &hmonitor,
                        physical_monitors.len() as u32,
                        physical_monitors.as_mut_ptr(),
                    ))
                    .ok()
                    {
                        return vec![Err(SysError::GetPhysicalMonitorsFailed(e))];
                    };
                    // Transform immediately into WrappedPhysicalMonitor so the handles don't leak
                    let physical_monitors = physical_monitors
                        .into_iter()
                        .map(|pm| {
                            (
                                WrappedPhysicalMonitor(pm.hPhysicalMonitor),
                                pm.szPhysicalMonitorDescription,
                            )
                        })
                        .collect::<Vec<_>>();

                    // Get the Display Devices in the HMONITOR
                    let mut device_number = 0;
                    let mut device = DISPLAY_DEVICEW::default();
                    device.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
                    let mut display_devices = Vec::new();
                    while EnumDisplayDevicesW(
                        PWSTR(info.szDevice.as_mut_ptr()),
                        device_number,
                        &mut device,
                        EDD_GET_DEVICE_INTERFACE_NAME,
                    )
                    .as_bool()
                    {
                        device_number += 1;
                        display_devices.push(device.clone());
                    }
                    if display_devices.len() != physical_monitors.len() {
                        // There doesn't seem to be anyway to directly associate a physical monitor handle
                        // with the equivalent display device, other than by array indexing
                        // https://stackoverflow.com/questions/63095216/how-to-associate-physical-monitor-with-monitor-deviceid
                        return vec![Err(SysError::EnumerationMismatch)];
                    }

                    physical_monitors
                        .into_iter()
                        .zip(display_devices)
                        .filter_map(|((physical_monitor, description), mut display_device)| {
                            // Get a file handle for this physical monitor
                            // Note this is a different type of handle
                            let device_name = wchar_to_string(&display_device.DeviceName);
                            let handle = CreateFileW(
                                PWSTR(display_device.DeviceID.as_mut_ptr()),
                                FILE_ACCESS_FLAGS(GENERIC_READ | GENERIC_WRITE),
                                FILE_SHARE_READ | FILE_SHARE_WRITE,
                                std::ptr::null_mut(),
                                OPEN_EXISTING,
                                FILE_FLAGS_AND_ATTRIBUTES(0),
                                HANDLE::NULL,
                            );
                            if handle.is_invalid() {
                                let e = HRESULT::from_thread();
                                // This error occurs for virtual devices e.g. Remote Desktop
                                // sessions - they are not real monitors
                                if e == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
                                    return None;
                                }
                                return Some(Err(
                                    SysError::OpeningMonitorDeviceInterfaceHandleFailed {
                                        device_name,
                                        source: windows::Error::from(e),
                                    },
                                ));
                            }
                            Some(Ok(Brightness {
                                physical_monitor,
                                file_handle: WrappedFileHandle(handle),
                                device_name,
                                device_description: wchar_to_string(&description),
                                device_string: wchar_to_string(&display_device.DeviceString),
                                device_id: wchar_to_string(&display_device.DeviceID),
                                device_key: wchar_to_string(&display_device.DeviceKey),
                            }))
                        })
                        .collect()
                }))
            }
        }
    };
    futures::stream::iter(devices)
}

#[derive(Debug, Error, Clone)]
pub enum SysError {
    #[error("Failed to enumerate device monitors")]
    EnumDeviceMonitorsFailed(#[source] windows::Error),
    #[error("Failed to get monitor info")]
    GetMonitorInfoFailed(#[source] windows::Error),
    #[error("Failed to get physical monitors from the HMONITOR")]
    GetPhysicalMonitorsFailed(#[source] windows::Error),
    #[error(
    "The length of GetPhysicalMonitorsFromHMONITOR() and EnumDisplayDevicesW() results did not \
     match, this likely means that monitors were connected/disconnected in between listing devices"
    )]
    EnumerationMismatch,
    #[error("Failed to open monitor interface handle (CreateFileW)")]
    OpeningMonitorDeviceInterfaceHandleFailed {
        device_name: String,
        #[source]
        source: windows::Error,
    },
    #[error("Failed to query supported brightness (IOCTL)")]
    IoctlQuerySupportedBrightnessFailed {
        device_name: String,
        #[source]
        source: windows::Error,
    },
    #[error("Failed to query display brightness (IOCTL)")]
    IoctlQueryDisplayBrightnessFailed {
        device_name: String,
        #[source]
        source: windows::Error,
    },
    #[error("Unexpected response when querying display brightness (IOCTL)")]
    IoctlQueryDisplayBrightnessUnexpectedResponse { device_name: String },
    #[error("Failed to get monitor brightness (DDCCI)")]
    GettingMonitorBrightnessFailed {
        device_name: String,
        #[source]
        source: windows::Error,
    },
    #[error("Failed to set monitor brightness (IOCTL)")]
    IoctlSetBrightnessFailed {
        device_name: String,
        #[source]
        source: windows::Error,
    },
    #[error("Failed to set monitor brightness (DDCCI)")]
    SettingBrightnessFailed {
        device_name: String,
        #[source]
        source: windows::Error,
    },
}

impl From<SysError> for Error {
    fn from(e: SysError) -> Self {
        match &e {
            SysError::EnumerationMismatch
            | SysError::GetPhysicalMonitorsFailed(..)
            | SysError::EnumDeviceMonitorsFailed(..)
            | SysError::GetMonitorInfoFailed(..)
            | SysError::OpeningMonitorDeviceInterfaceHandleFailed { .. } => {
                Error::ListingDevicesFailed(Box::new(e))
            }
            SysError::IoctlQuerySupportedBrightnessFailed { device_name, .. }
            | SysError::IoctlQueryDisplayBrightnessFailed { device_name, .. }
            | SysError::IoctlQueryDisplayBrightnessUnexpectedResponse { device_name }
            | SysError::GettingMonitorBrightnessFailed { device_name, .. } => {
                Error::GettingDeviceInfoFailed {
                    device: device_name.clone(),
                    source: Box::new(e),
                }
            }
            SysError::SettingBrightnessFailed { device_name, .. }
            | SysError::IoctlSetBrightnessFailed { device_name, .. } => {
                Error::SettingBrightnessFailed {
                    device: device_name.clone(),
                    source: Box::new(e),
                }
            }
        }
    }
}

pub fn wchar_to_string(s: &[u16]) -> String {
    let end = s.iter().position(|&x| x == 0).unwrap_or(s.len());
    let truncated = &s[0..end];
    OsString::from_wide(truncated).to_string_lossy().into()
}

#[derive(Debug, Default)]
struct DdcciBrightnessValues {
    min: u32,
    current: u32,
    max: u32,
}

impl DdcciBrightnessValues {
    fn get_current_percentage(&self) -> u32 {
        let normalised_max = (self.max - self.min) as f64;
        let normalised_current = (self.current - self.min) as f64;
        (normalised_current / normalised_max * 100.0).round() as u32
    }

    fn percentage_to_current(&self, percentage: u32) -> u32 {
        let normalised_max = (self.max - self.min) as f64;
        let fraction = percentage as f64 / 100.0;
        let normalised_current = fraction * normalised_max;
        normalised_current.round() as u32 + self.min
    }
}

fn ddcci_get_monitor_brightness(device: &Brightness) -> Result<DdcciBrightnessValues, SysError> {
    unsafe {
        let mut v = DdcciBrightnessValues::default();
        BOOL(GetMonitorBrightness(
            device.physical_monitor.0,
            &mut v.min,
            &mut v.current,
            &mut v.max,
        ))
        .ok()
        .map(|_| v)
        .map_err(|e| SysError::GettingMonitorBrightnessFailed {
            device_name: device.device_name.clone(),
            source: e,
        })
    }
}

fn ddcci_set_monitor_brightness(device: &Brightness, value: u32) -> Result<(), SysError> {
    unsafe {
        BOOL(SetMonitorBrightness(device.physical_monitor.0, value))
            .ok()
            .map_err(|e| SysError::SettingBrightnessFailed {
                device_name: device.device_name.clone(),
                source: e,
            })
    }
}

#[derive(Debug)]
// "Each level is a value from 0 to 100"
struct IoctlSupportedBrightnessLevels(Vec<u8>);

impl IoctlSupportedBrightnessLevels {
    fn get_nearest(&self, percentage: u32) -> u8 {
        self.0
            .iter()
            .min_by_key(|&num| (*num as i64 - percentage as i64).abs())
            .map(|x| *x)
            .unwrap_or(0)
    }
}

fn ioctl_query_supported_brightness(
    device: &Brightness,
) -> Result<IoctlSupportedBrightnessLevels, windows::Error> {
    unsafe {
        let mut bytes_returned = 0;
        let mut out_buffer = Vec::<u8>::with_capacity(256);
        DeviceIoControl(
            device.file_handle.0,
            IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS,
            std::ptr::null_mut(),
            0,
            out_buffer.as_mut_ptr() as *mut c_void,
            out_buffer.capacity() as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
        .ok()
        .map(|_| {
            out_buffer.set_len(bytes_returned as usize);
            IoctlSupportedBrightnessLevels(out_buffer)
        })
    }
}

fn ioctl_query_display_brightness(device: &Brightness) -> Result<u32, SysError> {
    unsafe {
        let mut bytes_returned = 0;
        let mut display_brightness = DISPLAY_BRIGHTNESS::default();
        DeviceIoControl(
            device.file_handle.0,
            IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS,
            std::ptr::null_mut(),
            0,
            (&mut display_brightness as *mut DISPLAY_BRIGHTNESS) as *mut c_void,
            std::mem::size_of::<DISPLAY_BRIGHTNESS>() as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
        .ok()
        .map_err(|e| SysError::IoctlQueryDisplayBrightnessFailed {
            device_name: device.device_name.clone(),
            source: e,
        })
        .and_then(|_| {
            if display_brightness.ucDisplayPolicy == DISPLAYPOLICY_AC as u8 {
                // This is a value between 0 and 100.
                Ok(display_brightness.ucACBrightness as u32)
            } else if display_brightness.ucDisplayPolicy == DISPLAYPOLICY_DC as u8 {
                // This is a value between 0 and 100.
                Ok(display_brightness.ucDCBrightness as u32)
            } else {
                Err(SysError::IoctlQueryDisplayBrightnessUnexpectedResponse {
                    device_name: device.device_name.clone(),
                })
            }
        })
    }
}

fn ioctl_set_display_brightness(device: &Brightness, value: u8) -> Result<(), SysError> {
    // Seems to currently be missing from metadata
    const DISPLAYPOLICY_BOTH: u8 = 3;
    unsafe {
        let mut display_brightness = DISPLAY_BRIGHTNESS {
            ucACBrightness: value,
            ucDCBrightness: value,
            ucDisplayPolicy: DISPLAYPOLICY_BOTH,
        };
        let mut bytes_returned = 0;
        DeviceIoControl(
            device.file_handle.0,
            IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS,
            (&mut display_brightness as *mut DISPLAY_BRIGHTNESS) as *mut c_void,
            std::mem::size_of::<DISPLAY_BRIGHTNESS>() as u32,
            std::ptr::null_mut(),
            0,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
        .ok()
        .map(|_| {
            // There is a bug where if the IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS is
            // called immediately after then it won't show the newly updated values
            // Doing a very tiny sleep seems to mitigate this
            std::thread::sleep(std::time::Duration::from_nanos(1));
            ()
        })
        .map_err(|e| SysError::IoctlSetBrightnessFailed {
            device_name: device.device_name.clone(),
            source: e,
        })
    }
}

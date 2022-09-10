// Copyright (C) 2022 The brightness project authors. Distributed under the 0BSD license.

//! Platform-specific implementation for Windows.

use crate::{blocking::BrightnessDevice, Error};
use itertools::Either;
use std::{
    collections::HashMap,
    ffi::{c_void, OsString},
    fmt,
    iter::once,
    mem::size_of,
    os::windows::ffi::OsStringExt,
    ptr,
};
use windows::{
    core::{Error as WinError, PCWSTR},
    Win32::{
        Devices::Display::{
            DestroyPhysicalMonitor, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes,
            GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
            GetPhysicalMonitorsFromHMONITOR, QueryDisplayConfig, SetMonitorBrightness,
            DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO,
            DISPLAYCONFIG_MODE_INFO_TYPE_TARGET, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL,
            DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_TARGET_DEVICE_NAME,
            DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY, DISPLAYPOLICY_AC, DISPLAYPOLICY_DC,
            DISPLAY_BRIGHTNESS, IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS,
            IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS, IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS,
            PHYSICAL_MONITOR,
        },
        Foundation::{
            CloseHandle, BOOL, ERROR_ACCESS_DENIED, ERROR_SUCCESS, HANDLE, LPARAM, RECT,
            WIN32_ERROR,
        },
        Graphics::Gdi::{
            EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW,
            DISPLAY_DEVICE_ACTIVE, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
            QDC_ONLY_ACTIVE_PATHS,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        },
        System::IO::DeviceIoControl,
        UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME,
    },
};

/// Windows-specific brightness functionality.
pub trait BrightnessExt {
    /// Returns device description
    fn device_description(&self) -> Result<String, Error>;

    /// Returns the device registry key
    fn device_registry_key(&self) -> Result<String, Error>;

    /// Returns the device path
    fn device_path(&self) -> Result<String, Error>;
}

#[derive(Debug)]
pub(crate) struct BlockingDeviceImpl {
    physical_monitor: WrappedPhysicalMonitor,
    file_handle: WrappedFileHandle,
    device_name: String,
    /// Note: PHYSICAL_MONITOR.szPhysicalMonitorDescription == DISPLAY_DEVICEW.DeviceString
    /// Description is **not** unique.
    pub(crate) device_description: String,
    pub(crate) device_key: String,
    /// Note: DISPLAYCONFIG_TARGET_DEVICE_NAME.monitorDevicePath == DISPLAY_DEVICEW.DeviceID (with EDD_GET_DEVICE_INTERFACE_NAME)\
    /// These are in the "DOS Device Path" format.
    pub(crate) device_path: String,
    output_technology: DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY,
}

impl BlockingDeviceImpl {
    fn is_internal(&self) -> bool {
        self.output_technology == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
    }
}

/// A safe wrapper for a physical monitor handle that implements `Drop` to call `DestroyPhysicalMonitor`
struct WrappedPhysicalMonitor(HANDLE);

impl fmt::Debug for WrappedPhysicalMonitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0 .0)
    }
}

impl Drop for WrappedPhysicalMonitor {
    fn drop(&mut self) {
        unsafe {
            DestroyPhysicalMonitor(self.0);
        }
    }
}

/// A safe wrapper for a windows HANDLE that implements `Drop` to call `CloseHandle`
struct WrappedFileHandle(HANDLE);

impl fmt::Debug for WrappedFileHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0 .0)
    }
}

impl Drop for WrappedFileHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[inline]
fn flag_set<T: std::ops::BitAnd<Output = T> + std::cmp::PartialEq + Copy>(t: T, flag: T) -> bool {
    t & flag == flag
}

impl crate::blocking::Brightness for BlockingDeviceImpl {
    fn device_name(&self) -> Result<String, Error> {
        Ok(self.device_name.clone())
    }

    fn get(&self) -> Result<u32, Error> {
        Ok(if self.is_internal() {
            ioctl_query_display_brightness(self)?
        } else {
            ddcci_get_monitor_brightness(self)?.get_current_percentage()
        })
    }

    fn set(&self, percentage: u32) -> Result<(), Error> {
        if self.is_internal() {
            let supported = ioctl_query_supported_brightness(self)?;
            let new_value = supported.get_nearest(percentage);
            ioctl_set_display_brightness(self, new_value)?;
        } else {
            let current = ddcci_get_monitor_brightness(self)?;
            let new_value = current.percentage_to_current(percentage);
            ddcci_set_monitor_brightness(self, new_value)?;
        }
        Ok(())
    }
}

pub(crate) fn brightness_devices() -> impl Iterator<Item = Result<BlockingDeviceImpl, SysError>> {
    unsafe {
        let device_info_map = match get_device_info_map() {
            Ok(info) => info,
            Err(e) => return Either::Right(once(Err(e))),
        };
        let hmonitors = match enum_display_monitors() {
            Ok(monitors) => monitors,
            Err(e) => return Either::Right(once(Err(e))),
        };
        Either::Left(hmonitors.into_iter().flat_map(move |hmonitor| {
            let physical_monitors = match get_physical_monitors_from_hmonitor(hmonitor) {
                Ok(p) => p,
                Err(e) => return vec![Err(e)],
            };
            let display_devices = match get_display_devices_from_hmonitor(hmonitor) {
                Ok(p) => p,
                Err(e) => return vec![Err(e)],
            };
            if display_devices.len() != physical_monitors.len() {
                // There doesn't seem to be any way to directly associate a physical monitor
                // handle with the equivalent display device, other than by array indexing
                // https://stackoverflow.com/questions/63095216/how-to-associate-physical-monitor-with-monitor-deviceid
                return vec![Err(SysError::EnumerationMismatch)];
            }
            physical_monitors
                .into_iter()
                .zip(display_devices)
                .filter_map(|(physical_monitor, display_device)| {
                    get_file_handle_for_display_device(&display_device)
                        .transpose()
                        .map(|file_handle| (physical_monitor, display_device, file_handle))
                })
                .map(|(physical_monitor, display_device, file_handle)| {
                    let file_handle = file_handle?;
                    let info = device_info_map
                        .get(&display_device.DeviceID)
                        .ok_or(SysError::DeviceInfoMissing)?;
                    Ok(BlockingDeviceImpl {
                        physical_monitor,
                        file_handle,
                        device_name: wchar_to_string(&display_device.DeviceName),
                        device_description: wchar_to_string(&display_device.DeviceString),
                        device_key: wchar_to_string(&display_device.DeviceKey),
                        device_path: wchar_to_string(&display_device.DeviceID),
                        output_technology: info.outputTechnology,
                    })
                })
                .collect()
        }))
    }
}

/// Returns a `HashMap` of Device Path to `DISPLAYCONFIG_TARGET_DEVICE_NAME`.\
/// This can be used to find the `DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY` for a monitor.\
/// The output technology is used to determine if a device is internal or external.
unsafe fn get_device_info_map(
) -> Result<HashMap<[u16; 128], DISPLAYCONFIG_TARGET_DEVICE_NAME>, SysError> {
    let mut path_count = 0;
    let mut mode_count = 0;
    check_status(
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count),
        SysError::GetDisplayConfigBufferSizesFailed,
    )?;
    let mut display_paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut display_modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
    check_status(
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            display_paths.as_mut_ptr(),
            &mut mode_count,
            display_modes.as_mut_ptr(),
            std::ptr::null_mut(),
        ),
        SysError::QueryDisplayConfigFailed,
    )?;
    display_modes
        .into_iter()
        .filter(|mode| mode.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_TARGET)
        .flat_map(|mode| {
            let mut device_name = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            device_name.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            device_name.header.adapterId = mode.adapterId;
            device_name.header.id = mode.id;
            device_name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            let result = to_win32_error(DisplayConfigGetDeviceInfo(&mut device_name.header));
            match result {
                ERROR_SUCCESS => Some(Ok((device_name.monitorDevicePath, device_name))),
                // This error occurs if the calling process does not have access to the current desktop or is running on a remote session.
                ERROR_ACCESS_DENIED => None,
                _ => Some(Err(SysError::DisplayConfigGetDeviceInfoFailed(
                    result.into(),
                ))),
            }
        })
        .collect()
}

/// Calls `EnumDisplayMonitors` and returns a list of `HMONITOR` handles.\
/// Note that a `HMONITOR` is a logical construct that may correspond to multiple physical monitors.\
/// e.g. when in "Duplicate" mode two physical monitors will belong to the same `HMONITOR`
unsafe fn enum_display_monitors() -> Result<Vec<HMONITOR>, SysError> {
    unsafe extern "system" fn enum_monitors(
        handle: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(data.0 as *mut Vec<HMONITOR>);
        monitors.push(handle);
        true.into()
    }
    let mut hmonitors = Vec::<HMONITOR>::new();
    EnumDisplayMonitors(
        HDC::default(),
        ptr::null_mut(),
        Some(enum_monitors),
        LPARAM(&mut hmonitors as *mut _ as isize),
    )
    .ok()
    .map_err(SysError::EnumDisplayMonitorsFailed)?;
    Ok(hmonitors)
}

/// Gets the list of `PHYSICAL_MONITOR` handles that belong to a `HMONITOR`.\
/// These handles are required for use with the DDC/CI functions, however a valid handle will still
/// be returned for non DDC/CI monitors and also Remote Desktop Session displays.\
/// Also note that physically connected but disabled (inactive) monitors are not returned from this API.
unsafe fn get_physical_monitors_from_hmonitor(
    hmonitor: HMONITOR,
) -> Result<Vec<WrappedPhysicalMonitor>, SysError> {
    let mut physical_number: u32 = 0;
    BOOL(GetNumberOfPhysicalMonitorsFromHMONITOR(
        hmonitor,
        &mut physical_number,
    ))
    .ok()
    .map_err(SysError::GetPhysicalMonitorsFailed)?;
    let mut raw_physical_monitors = vec![PHYSICAL_MONITOR::default(); physical_number as usize];
    // Allocate first so that pushing the wrapped handles always succeeds.
    let mut physical_monitors = Vec::with_capacity(raw_physical_monitors.len());
    BOOL(GetPhysicalMonitorsFromHMONITOR(
        hmonitor,
        &mut raw_physical_monitors,
    ))
    .ok()
    .map_err(SysError::GetPhysicalMonitorsFailed)?;
    // Transform immediately into WrappedPhysicalMonitor so the handles don't leak
    raw_physical_monitors
        .into_iter()
        .for_each(|pm| physical_monitors.push(WrappedPhysicalMonitor(pm.hPhysicalMonitor)));
    Ok(physical_monitors)
}

/// Gets the list of display devices that belong to a `HMONITOR`.\
/// Due to the `EDD_GET_DEVICE_INTERFACE_NAME` flag, the `DISPLAY_DEVICEW` will contain the DOS
/// device path for each monitor in the `DeviceID` field.\
/// Note: Connected but inactive displays have been filtered out.
unsafe fn get_display_devices_from_hmonitor(
    hmonitor: HMONITOR,
) -> Result<Vec<DISPLAY_DEVICEW>, SysError> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let info_ptr = &mut info as *mut _ as *mut MONITORINFO;
    GetMonitorInfoW(hmonitor, info_ptr)
        .ok()
        .map_err(SysError::GetMonitorInfoFailed)?;
    Ok((0..)
        .map_while(|device_number| {
            let mut device = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            EnumDisplayDevicesW(
                PCWSTR(info.szDevice.as_ptr()),
                device_number,
                &mut device,
                EDD_GET_DEVICE_INTERFACE_NAME,
            )
            .as_bool()
            .then(|| device)
        })
        .filter(|device| flag_set(device.StateFlags, DISPLAY_DEVICE_ACTIVE))
        .collect())
}

/// Opens and returns a file handle for a display device using its DOS device path.\
/// These handles are only used for the `DeviceIoControl` API (for internal displays); a
/// handle can still be returned for external displays, but it should not be used.\
/// A `None` value means that a handle could not be opened, but this was for an expected reason,
/// indicating this display device should be skipped.
unsafe fn get_file_handle_for_display_device(
    display_device: &DISPLAY_DEVICEW,
) -> Result<Option<WrappedFileHandle>, SysError> {
    CreateFileW(
        PCWSTR(display_device.DeviceID.as_ptr()),
        FILE_GENERIC_READ | FILE_GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        ptr::null_mut(),
        OPEN_EXISTING,
        Default::default(),
        HANDLE::default(),
    )
    .map(|h| Some(WrappedFileHandle(h)))
    .or_else(|e| {
        // This error occurs for virtual devices e.g. Remote Desktop
        // sessions - they are not real monitors
        (e.code() == ERROR_ACCESS_DENIED.to_hresult())
            .then_some(None)
            .ok_or_else(|| SysError::OpeningMonitorDeviceInterfaceHandleFailed {
                device_name: wchar_to_string(&display_device.DeviceName),
                source: e,
            })
    })
}

#[derive(Clone, Debug, Error)]
pub(crate) enum SysError {
    #[error("Failed to enumerate device monitors")]
    EnumDisplayMonitorsFailed(#[source] WinError),
    #[error("Failed to get display config buffer sizes")]
    GetDisplayConfigBufferSizesFailed(#[source] WinError),
    #[error("Failed to query display config")]
    QueryDisplayConfigFailed(#[source] WinError),
    #[error("Failed to get display config device info")]
    DisplayConfigGetDeviceInfoFailed(#[source] WinError),
    #[error("Failed to get monitor info")]
    GetMonitorInfoFailed(#[source] WinError),
    #[error("Failed to get physical monitors from the HMONITOR")]
    GetPhysicalMonitorsFailed(#[source] WinError),
    #[error(
    "The length of GetPhysicalMonitorsFromHMONITOR() and EnumDisplayDevicesW() results did not \
     match, this could be because monitors were connected/disconnected while loading devices"
    )]
    EnumerationMismatch,
    #[error(
    "Unable to find a matching device info for this display device, this could be because monitors \
     were connected while loading devices"
    )]
    DeviceInfoMissing,
    #[error("Failed to open monitor interface handle (CreateFileW)")]
    OpeningMonitorDeviceInterfaceHandleFailed {
        device_name: String,
        source: WinError,
    },
    #[error("Failed to query supported brightness (IOCTL)")]
    IoctlQuerySupportedBrightnessFailed {
        device_name: String,
        source: WinError,
    },
    #[error("Failed to query display brightness (IOCTL)")]
    IoctlQueryDisplayBrightnessFailed {
        device_name: String,
        source: WinError,
    },
    #[error("Unexpected response when querying display brightness (IOCTL)")]
    IoctlQueryDisplayBrightnessUnexpectedResponse { device_name: String },
    #[error("Failed to get monitor brightness (DDCCI)")]
    GettingMonitorBrightnessFailed {
        device_name: String,
        source: WinError,
    },
    #[error("Failed to set monitor brightness (IOCTL)")]
    IoctlSetBrightnessFailed {
        device_name: String,
        source: WinError,
    },
    #[error("Failed to set monitor brightness (DDCCI)")]
    SettingBrightnessFailed {
        device_name: String,
        source: WinError,
    },
}

impl From<SysError> for Error {
    fn from(e: SysError) -> Self {
        match &e {
            SysError::EnumerationMismatch
            | SysError::DeviceInfoMissing
            | SysError::GetDisplayConfigBufferSizesFailed(..)
            | SysError::QueryDisplayConfigFailed(..)
            | SysError::DisplayConfigGetDeviceInfoFailed(..)
            | SysError::GetPhysicalMonitorsFailed(..)
            | SysError::EnumDisplayMonitorsFailed(..)
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

fn wchar_to_string(s: &[u16]) -> String {
    let end = s.iter().position(|&x| x == 0).unwrap_or(s.len());
    let truncated = &s[0..end];
    OsString::from_wide(truncated).to_string_lossy().into()
}

fn to_win32_error(status: i32) -> WIN32_ERROR {
    WIN32_ERROR(status as u32)
}

fn check_status<F, E>(status: i32, f: F) -> Result<(), E>
where
    F: FnOnce(WinError) -> E,
{
    to_win32_error(status).ok().map_err(f)
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

fn ddcci_get_monitor_brightness(
    device: &BlockingDeviceImpl,
) -> Result<DdcciBrightnessValues, SysError> {
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

fn ddcci_set_monitor_brightness(device: &BlockingDeviceImpl, value: u32) -> Result<(), SysError> {
    unsafe {
        BOOL(SetMonitorBrightness(device.physical_monitor.0, value))
            .ok()
            .map_err(|e| SysError::SettingBrightnessFailed {
                device_name: device.device_name.clone(),
                source: e,
            })
    }
}

/// Each level is a value from 0 to 100
#[derive(Debug)]
struct IoctlSupportedBrightnessLevels(Vec<u8>);

impl IoctlSupportedBrightnessLevels {
    fn get_nearest(&self, percentage: u32) -> u8 {
        self.0
            .iter()
            .copied()
            .min_by_key(|&num| (num as i64 - percentage as i64).abs())
            .unwrap_or(0)
    }
}

fn ioctl_query_supported_brightness(
    device: &BlockingDeviceImpl,
) -> Result<IoctlSupportedBrightnessLevels, SysError> {
    unsafe {
        let mut bytes_returned = 0;
        let mut out_buffer = Vec::<u8>::with_capacity(256);
        DeviceIoControl(
            device.file_handle.0,
            IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS,
            ptr::null_mut(),
            0,
            out_buffer.as_mut_ptr() as *mut c_void,
            out_buffer.capacity() as u32,
            &mut bytes_returned,
            ptr::null_mut(),
        )
        .ok()
        .map(|_| {
            out_buffer.set_len(bytes_returned as usize);
            IoctlSupportedBrightnessLevels(out_buffer)
        })
        .map_err(|e| SysError::IoctlQuerySupportedBrightnessFailed {
            device_name: device.device_name.clone(),
            source: e,
        })
    }
}

fn ioctl_query_display_brightness(device: &BlockingDeviceImpl) -> Result<u32, SysError> {
    unsafe {
        let mut bytes_returned = 0;
        let mut display_brightness = DISPLAY_BRIGHTNESS::default();
        DeviceIoControl(
            device.file_handle.0,
            IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS,
            ptr::null_mut(),
            0,
            &mut display_brightness as *mut DISPLAY_BRIGHTNESS as *mut c_void,
            size_of::<DISPLAY_BRIGHTNESS>() as u32,
            &mut bytes_returned,
            ptr::null_mut(),
        )
        .ok()
        .map_err(|e| SysError::IoctlQueryDisplayBrightnessFailed {
            device_name: device.device_name.clone(),
            source: e,
        })
        .and_then(|_| match display_brightness.ucDisplayPolicy as u32 {
            DISPLAYPOLICY_AC => {
                // This is a value between 0 and 100.
                Ok(display_brightness.ucACBrightness as u32)
            }
            DISPLAYPOLICY_DC => {
                // This is a value between 0 and 100.
                Ok(display_brightness.ucDCBrightness as u32)
            }
            _ => Err(SysError::IoctlQueryDisplayBrightnessUnexpectedResponse {
                device_name: device.device_name.clone(),
            }),
        })
    }
}

fn ioctl_set_display_brightness(device: &BlockingDeviceImpl, value: u8) -> Result<(), SysError> {
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
            &mut display_brightness as *mut DISPLAY_BRIGHTNESS as *mut c_void,
            size_of::<DISPLAY_BRIGHTNESS>() as u32,
            ptr::null_mut(),
            0,
            &mut bytes_returned,
            ptr::null_mut(),
        )
        .ok()
        .map(|_| {
            // There is a bug where if the IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS is
            // called immediately after then it won't show the newly updated values
            // Doing a very tiny sleep seems to mitigate this
            std::thread::sleep(std::time::Duration::from_nanos(1));
        })
        .map_err(|e| SysError::IoctlSetBrightnessFailed {
            device_name: device.device_name.clone(),
            source: e,
        })
    }
}

impl BrightnessExt for BrightnessDevice {
    fn device_description(&self) -> Result<String, Error> {
        Ok(self.0.device_description.clone())
    }

    fn device_registry_key(&self) -> Result<String, Error> {
        Ok(self.0.device_key.clone())
    }

    fn device_path(&self) -> Result<String, Error> {
        Ok(self.0.device_path.clone())
    }
}

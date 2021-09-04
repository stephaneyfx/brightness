fn main() {
    windows::build! {
        Windows::Win32::Devices::Display::{
            DestroyPhysicalMonitor, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
            GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, DISPLAYPOLICY_AC,
            DISPLAYPOLICY_DC, DISPLAY_BRIGHTNESS, IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS,
            IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS, IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS,
        },
        Windows::Win32::Foundation::{CloseHandle, BOOL, HANDLE, LPARAM, PWSTR, RECT},
        Windows::Win32::Graphics::Gdi::{
            EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW, HDC,
            HMONITOR, MONITORINFO, MONITORINFOEXW,
        },
        Windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_ACCESS_FLAGS, FILE_FLAGS_AND_ATTRIBUTES,
        },
        Windows::Win32::System::Diagnostics::Debug::WIN32_ERROR,
        Windows::Win32::System::SystemServices::{DeviceIoControl, GENERIC_READ, GENERIC_WRITE},
        Windows::Win32::UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME,
    };
}

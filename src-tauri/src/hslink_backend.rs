use hidapi;
use lazy_static::lazy_static;
use std::string::ToString;
use std::sync::Mutex;

lazy_static! {
    static ref HID_API: Mutex<hidapi::HidApi> =
        Mutex::new(hidapi::HidApi::new().expect("Failed to create HidApi instance"));
    static ref HSLink_VID: u16 = 0x0D28;
    static ref HSLink_PID: u16 = 0x0204;
    static ref HSLink_MANUFACTURER: String = String::from("CherryUSB");
    static ref HSLink_DEVICE: Mutex<Option<hidapi::HidDevice>> = Mutex::new(None);
    static ref HSLink_DONW_REPORT_ID: u8 = 0x01;
    static ref HSLink_UP_REPORT_ID: u8 = 0x02;
}

#[derive(Debug, thiserror::Error)]
// add HSLinkError to let frontend to handle it
pub enum HSLinkError {
    #[error("HSLinkError: Device not found")]
    DeviceNotFound,
    #[error("HSLinkError: Device not opened")]
    DeviceNotOpened,
    #[error("HSLinkError: Write error")]
    WriteErr,
    #[error("HSLinkError: Read error")]
    ReadErr,
    #[error("HSLinkError: Response error")]
    RspErr,
    #[error("HSLinkError: NotSupport")]
    NotSupport,
}

impl serde::Serialize for HSLinkError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

#[tauri::command]
pub fn hslink_list_device() -> Vec<String> {
    let mut devices: Vec<String> = Vec::new();
    let mut hid_api = HID_API.lock().unwrap();
    hid_api.refresh_devices().unwrap();
    for device_info in hid_api.device_list() {
        if device_info.vendor_id() == *HSLink_VID
            && device_info.product_id() == *HSLink_PID
            && device_info.manufacturer_string().unwrap_or("") == *HSLink_MANUFACTURER
        {
            println!("Device Found:");
            println!("  Vendor ID: {:04X}", device_info.vendor_id());
            println!("  Product ID: {:04X}", device_info.product_id());
            println!("  Path: {:?}", device_info.path());
            println!("  Serial Number: {:?}", device_info.serial_number());
            if !device_info.serial_number().is_none() {
                devices.push(device_info.serial_number().unwrap().to_string());
            }
        }
    }
    if devices.is_empty() {
        println!("No HSLink devices found");
    }
    devices
}

#[tauri::command]
pub fn hslink_open_device(serial_number: String) -> Result<String, HSLinkError> {
    let mut device_lock = HSLink_DEVICE.lock().unwrap();
    let mut hid_api = HID_API.lock().unwrap();
    for device_info in hid_api.device_list() {
        if device_info.vendor_id() == *HSLink_VID && device_info.product_id() == *HSLink_PID {
            if device_info.serial_number().unwrap().to_string() == serial_number {
                match hid_api.open_path(device_info.path()) {
                    Ok(device) => {
                        println!("Device Opened: {:?}", device);
                        let sn = device_info.serial_number().unwrap().to_string();
                        if sn != serial_number {
                            return Err(HSLinkError::DeviceNotFound);
                        }
                        *device_lock = Some(device);
                        return Ok(sn);
                    }
                    Err(err) => {
                        println!("Error opening device: {:?}", err);
                        return Err(HSLinkError::DeviceNotOpened);
                    }
                }
            }
        }
    }
    Err(HSLinkError::DeviceNotFound)
}

#[tauri::command]
pub fn hslink_write(data: Vec<u8>) -> Result<(), HSLinkError> {
    let mut device_lock = HSLink_DEVICE.lock().unwrap();
    let mut buff = vec![0u8; data.len() + 1];
    buff[0] = *HSLink_DONW_REPORT_ID;
    (&mut buff[1..]).copy_from_slice(&data);
    if let Some(ref mut device) = *device_lock {
        match device.write(&buff) {
            Ok(res) => {
                println!("Wrote: {:?} byte(s)", res);
                Ok(())
            }
            Err(err) => {
                println!("Error writing to device: {:?}", err);
                Err(HSLinkError::WriteErr)
            }
        }
    } else {
        println!("Device not opened");
        Err(HSLinkError::DeviceNotOpened)
    }
}

#[tauri::command]
pub fn hslink_write_wait_rsp(data: Vec<u8>, timeout: u32) -> Result<String, HSLinkError> {
    let write_err = hslink_write(data);
    if write_err.is_err() {
        return Err(write_err.unwrap_err());
    }
    let mut device_lock = HSLink_DEVICE.lock().unwrap();
    let mut recv_buf = [0u8; 1024];
    if let Some(ref mut device) = *device_lock {
        match device.read_timeout(&mut recv_buf, timeout as i32) {
            Ok(res) => {
                println!("Read: {:?}", res);
                if recv_buf[0] == *HSLink_UP_REPORT_ID {
                    // find the first \0
                    let mut i = 0;
                    while i < res && recv_buf[i] != 0 {
                        i += 1;
                    }
                    let data = recv_buf[1..i].to_vec();
                    println!("Received: {:?}", data);
                    Ok(String::from_utf8(data).unwrap().to_string())
                } else {
                    Err(HSLinkError::RspErr)
                }
            }
            Err(err) => {
                println!("Error reading from device: {:?}", err);
                Err(HSLinkError::ReadErr)
            }
        }
    } else {
        Err(HSLinkError::DeviceNotOpened)
    }
}

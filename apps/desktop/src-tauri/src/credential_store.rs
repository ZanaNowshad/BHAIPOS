use uuid::Uuid;

const SERVICE:&str="com.bhaipos.desktop.device";

#[cfg(target_os="windows")]
pub fn write_device_secret(device_id:Uuid,secret:&str)->Result<(),String>{
    keyring::Entry::new(SERVICE,&device_id.to_string()).map_err(|error|error.to_string())?.set_password(secret).map_err(|error|error.to_string())
}

#[cfg(target_os="windows")]
pub fn read_device_secret(device_id:Uuid)->Result<String,String>{
    keyring::Entry::new(SERVICE,&device_id.to_string()).map_err(|error|error.to_string())?.get_password().map_err(|error|error.to_string())
}

#[cfg(target_os="windows")]
pub fn delete_device_secret(device_id:Uuid)->Result<(),String>{
    keyring::Entry::new(SERVICE,&device_id.to_string()).map_err(|error|error.to_string())?.delete_credential().map_err(|error|error.to_string())
}

#[cfg(not(target_os="windows"))]
pub fn write_device_secret(_device_id:Uuid,_secret:&str)->Result<(),String>{Err("OS-protected device credential storage is only available in the supported Windows build".into())}
#[cfg(not(target_os="windows"))]
pub fn read_device_secret(_device_id:Uuid)->Result<String,String>{Err("OS-protected device credential storage is only available in the supported Windows build".into())}
#[cfg(not(target_os="windows"))]
pub fn delete_device_secret(_device_id:Uuid)->Result<(),String>{Ok(())}

//! 根 CA 管理：首次生成、持久化、加载为 hudsucker 的签发机构。

use hudsucker::certificate_authority::RcgenAuthority;
use hudsucker::rustls;
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum CaError {
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("证书生成/解析失败：{0}")]
    Rcgen(#[from] rcgen::Error),
}

pub const CA_NAME: &str = "WXdown Proxy Root CA";

/// CA 材料（PEM）。
pub struct CaMaterial {
    pub key_pem: String,
    pub cert_pem: String,
}

/// 数据目录里有就加载，没有就生成一套新的并落盘。
pub fn ensure_ca(dir: &Path) -> Result<CaMaterial, CaError> {
    let key_path = dir.join("proxy-ca.key");
    let crt_path = dir.join("proxy-ca.crt");
    if key_path.exists() && crt_path.exists() {
        let key_pem = std::fs::read_to_string(&key_path)?;
        let cert_pem = std::fs::read_to_string(&crt_path)?;
        // 校验可用（坏了就重新生成）
        if try_authority(&cert_pem, &key_pem).is_ok() {
            return Ok(CaMaterial { key_pem, cert_pem });
        }
    }
    std::fs::create_dir_all(dir)?;

    let key = KeyPair::generate()?;
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, CA_NAME);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    // 10 年有效期（time 宏生成 OffsetDateTime）
    params.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(1);
    params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(3650);
    let cert = params.self_signed(&key)?;

    let material = CaMaterial {
        key_pem: key.serialize_pem(),
        cert_pem: cert.pem(),
    };
    // 私钥文件不希望被别的用户读到
    let _ = std::fs::set_permissions(&key_path, private_perms());
    std::fs::write(&key_path, &material.key_pem)?;
    std::fs::write(&crt_path, &material.cert_pem)?;
    let _ = std::fs::set_permissions(&key_path, private_perms());
    Ok(material)
}

#[cfg(not(windows))]
fn private_perms() -> std::fs::Permissions {
    use std::os::unix::fs::PermissionsExt;
    std::fs::Permissions::from_mode(0o600)
}

#[cfg(windows)]
fn private_perms() -> std::fs::Permissions {
    // Windows 上 ACL 继承数据目录本身的权限,这里返回只读默认值占位(不额外收紧)
    std::fs::metadata(&std::env::temp_dir())
        .map(|m| m.permissions())
        .unwrap_or_else(|_| std::fs::metadata(".").map(|m| m.permissions()).expect("取权限"))
}

fn try_authority(cert_pem: &str, key_pem: &str) -> Result<RcgenAuthority, CaError> {
    let key = KeyPair::from_pem(key_pem)?;
    let issuer = Issuer::from_ca_cert_pem(cert_pem, key)?;
    Ok(RcgenAuthority::new(
        issuer,
        1000,
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
}

/// 把 CA 材料装成代理签发机构。
pub fn build_authority(ca: &CaMaterial) -> Result<RcgenAuthority, CaError> {
    try_authority(&ca.cert_pem, &ca.key_pem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_and_reloads() {
        let dir = std::env::temp_dir().join(format!("wxdown-ca-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ca = ensure_ca(&dir).expect("生成 CA");
        assert!(ca.cert_pem.contains("BEGIN CERTIFICATE"));
        // 第二次读盘并构建成功
        let ca2 = ensure_ca(&dir).expect("复用 CA");
        assert_eq!(ca.cert_pem, ca2.cert_pem);
        assert!(build_authority(&ca2).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

use serde_derive::{Deserialize, Serialize};

// Generated by <https://app.quicktype.io>
// Modified slightly to make some JSON structure elements non-optional
// JSON record types for CertStream messages

#[derive(Serialize, Deserialize)]
pub struct CertStream {
  pub data: Data,
  pub message_type: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Data {
  pub cert_index: Option<i64>,
  pub cert_link: Option<String>,
  pub leaf_cert: LeafCert,
  pub seen: Option<f64>,
  pub source: Option<Source>,
  pub update_type: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct LeafCert {
  pub all_domains: Vec<String>,
  pub extensions: Option<Extensions>,
  pub fingerprint: Option<String>,
  pub issuer: Option<Issuer>,
  pub not_after: Option<i64>,
  pub not_before: Option<i64>,
  pub serial_number: Option<String>,
  pub signature_algorithm: Option<String>,
  pub subject: Option<Issuer>,
}

#[derive(Serialize, Deserialize)]
pub struct Extensions {
  #[serde(rename = "authorityInfoAccess")]
  pub authority_info_access: Option<String>,
  #[serde(rename = "authorityKeyIdentifier")]
  pub authority_key_identifier: Option<String>,
  #[serde(rename = "basicConstraints")]
  pub basic_constraints: Option<String>,
  #[serde(rename = "certificatePolicies")]
  pub certificate_policies: Option<String>,
  #[serde(rename = "ctlSignedCertificateTimestamp")]
  pub ctl_signed_certificate_timestamp: Option<String>,
  #[serde(rename = "extendedKeyUsage")]
  pub extended_key_usage: Option<String>,
  #[serde(rename = "keyUsage")]
  pub key_usage: Option<String>,
  #[serde(rename = "subjectAltName")]
  pub subject_alt_name: Option<String>,
  #[serde(rename = "subjectKeyIdentifier")]
  pub subject_key_identifier: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Issuer {
  #[serde(rename = "C")]
  pub c: Option<String>,
  #[serde(rename = "CN")]
  pub cn: Option<String>,
  #[serde(rename = "L")]
  pub l: Option<serde_json::Value>,
  #[serde(rename = "O")]
  pub o: Option<String>,
  #[serde(rename = "OU")]
  pub ou: Option<serde_json::Value>,
  #[serde(rename = "ST")]
  pub st: Option<serde_json::Value>,
  pub aggregated: Option<String>,
  #[serde(rename = "emailAddress")]
  pub email_address: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
pub struct Source {
  pub name: Option<String>,
  pub url: Option<String>,
}

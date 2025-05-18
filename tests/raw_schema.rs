use certstream_rust::json_types::CertStream;
use deltalake::arrow::datatypes::{DataType, Schema as ArrowSchema};
use std::convert::TryFrom;

#[test]
fn raw_schema_has_raw_string_field() {
    let schema = CertStream::raw_schema();
    let arrow_schema = ArrowSchema::try_from(&schema).expect("convert schema");
    let fields = arrow_schema.fields();
    assert_eq!(fields.len(), 1, "schema should have one field");
    let field = &fields[0];
    assert_eq!(field.name(), "raw");
    assert_eq!(field.data_type(), &DataType::Utf8);
}

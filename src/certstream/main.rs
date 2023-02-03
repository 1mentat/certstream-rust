// usage e.g. certstream --dbpath=~/Data/cert_doms.1

use std::process;
use std::str;
use std::{thread, time};

use clap::{Parser};
use json_types::CertStream;

use url::{Url}; // tokio* uses real URLs
use tokio::io::{Result}; 
use tokio_tungstenite::{connect_async}; 
use futures_util::{StreamExt}; 

// deltalake db
use deltalake::action::*;
use deltalake::arrow::array::*;
use deltalake::arrow::record_batch::RecordBatch;
use deltalake::writer::{DeltaWriter, RecordBatchWriter};
use deltalake::*;

use object_store::path::Path;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

// JSON record types for CertStream messages
mod json_types;

const CERTSTREAM_URL: &'static str = "wss://certstream.calidog.io/";
const WAIT_AFTER_DISCONNECT: u64 =  5; // seconds

// in the deserialization part, the type of the returnd, parsed JSON gets wonky
macro_rules! assert_types {
  ($($var:ident : $ty:ty),*) => { $(let _: & $ty = & $var;)* }
}

// Extract all domains from a CertStream-compatible CTL websockets server to RocksDB
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {

  // server to use (defaults to CertStream's)
  #[clap(short, long, default_value_t = String::from(CERTSTREAM_URL))]
  server: String,

  // how long to wait to connect after a remote disconnect
  #[clap(short, long, default_value_t = WAIT_AFTER_DISCONNECT)]
  patience: u64
  
}

// going all async b/c I may turn this into a library
#[tokio::main]
async fn main() -> Result<()> {

  let args = Args::parse();

  // may want todo someting on ^C
  ctrlc::set_handler(move || {
    process::exit(0x0000);
  }).expect("Error setting Ctrl-C handler");

  loop { // server is likely to drop connections
    //let batch_size = 20000;
    //let mut records = vec![];

    let records_vec = Arc::new(Mutex::new(Vec::<String>::new()));

    let table_uri = std::env::var("TABLE_URI").unwrap();

    let table_path = Path::from(table_uri.as_ref());

    let maybe_table = deltalake::open_table(&table_path).await;
    let mut table = match maybe_table {
        Ok(table) => table,
        Err(DeltaTableError::NotATable(_)) => {
            create_initialized_table(&table_path).await
        }
        Err(err) => Err(err).unwrap(),
    };

    let mut writer =
        RecordBatchWriter::for_table(&table).expect("Failed to make RecordBatchWriter");
    
    let certstream_url = Url::parse(args.server.as_str()).unwrap(); // we need an actual Url type
    
    // connect to CertStream's encrypted websocket interface 
    let (wss_stream, _response) = connect_async(certstream_url).await.expect("Failed to connect");
    
    // the WebSocketStrem has sink/stream (read/srite) components; this is how we get to them
    let (mut _write, read) = wss_stream.split();
    
    // process messages as they come in
    let read_future = read.for_each(|message| async {
      match message {

        Ok(msg) => { // we have the websockets message bytes as a str
           
          if let Ok(json_data) = msg.to_text() { // did the bytes convert to text ok?
            if json_data.len() > 0 { // do we actually have semi-valid JSON?
              match serde_json::from_str(&json_data) { // if deserialization works
                Ok(record) => { // then derserialize JSON
                  
                  assert_types! { record: json_types::CertStream }
                  records_vec.lock().unwrap().push(serde_json::to_string(&record).unwrap())
                }
                
                Err(err) => { eprintln!("{}", err) }
                
              }
            }
          }
        }
        
        Err(err) => { eprintln!("{}", err) }
        
      }
      
    });
    
    read_future.await;

    let batch = convert_to_batch(&table,records_vec.lock().unwrap().to_vec());

    writer.write(batch).await.unwrap();
    
    eprintln!("Server disconnected…waiting {} seconds and retrying…", args.patience);

    let _ = writer
    .flush_and_commit(&mut table)
    .await
    .expect("Failed to flush write");

    // wait for a bit to be kind to the server
    thread::sleep(time::Duration::from_secs(args.patience));
    
  }
  
}

fn convert_to_batch(table: &DeltaTable, records: Vec<std::string::String>) -> RecordBatch {
  let metadata = table
      .get_metadata()
      .expect("Failed to get metadata for the table");
  let arrow_schema = <deltalake::arrow::datatypes::Schema as TryFrom<&Schema>>::try_from(
      &metadata.schema.clone(),
  )
  .expect("Failed to convert to arrow schema");
  let arrow_schema_ref = Arc::new(arrow_schema);

  let string_refs: Vec<&str> = records.iter().map(|r| &r[..]).collect();

  let arrow_array: Vec<Arc<dyn Array>> = vec![
      Arc::new(StringArray::from(string_refs)),
  ];

  RecordBatch::try_new(arrow_schema_ref, arrow_array).expect("Failed to create RecordBatch")
}

async fn create_initialized_table(table_path: &Path) -> DeltaTable {
  let mut table = DeltaTableBuilder::from_uri(table_path).build().unwrap();
  let table_schema = CertStream::raw_schema();
  let mut commit_info = serde_json::Map::<String, serde_json::Value>::new();
  commit_info.insert(
      "operation".to_string(),
      serde_json::Value::String("CREATE TABLE".to_string()),
  );
  commit_info.insert(
      "userName".to_string(),
      serde_json::Value::String("test user".to_string()),
  );

  let protocol = Protocol {
      min_reader_version: 1,
      min_writer_version: 1,
  };

  let metadata = DeltaTableMetaData::new(None, None, None, table_schema, vec![], HashMap::new());

  table
      .create(metadata, protocol, Some(commit_info), None)
      .await
      .unwrap();

  table
}
extern crate aw_client_rust;
extern crate byteorder;

use std::env;
use std::io::prelude::*;
use std::os::unix::net::UnixStream;

use byteorder::{WriteBytesExt, ReadBytesExt, LittleEndian};

use chrono::prelude::*;
use chrono::DateTime;

use serde_json::{Value, Map};

fn sway_ipc_subscribe() -> Vec<u8> {
    let mut bytearr : Vec<u8> = vec![];

    /* magic bytes */
    bytearr.append(&mut b"i3-ipc".to_vec());
    let mut message = b"['window']".to_vec();

    /* u32 payload size */
    bytearr.write_u32::<LittleEndian>(message.len() as u32).unwrap();

    /* u32 command */
    bytearr.write_u32::<LittleEndian>(2u32).unwrap();

    /* payload */
    bytearr.append(&mut message);

    return bytearr;
}

fn get_next_message(stream: &mut UnixStream) -> String {
    /* magic bytes */
    let mut magic_buffer = [0; 6];
    stream.read(&mut magic_buffer).unwrap();
    //println!("{}", std::str::from_utf8(&magic_buffer).unwrap());

    /* u32 payload size */
    let mut event_len = [0; 4];
    stream.read(&mut event_len).unwrap();
    use std::io::Cursor;
    let event_len = Cursor::new(event_len).read_u32::<LittleEndian>().unwrap();
    //println!("{}", event_len);

    /* u32 command */
    let mut event_type = [0; 4];
    stream.read(&mut event_type).unwrap();

    /* payload */
    let mut buffer = Vec::new();
    for _ in 0..event_len {
        buffer.push(0u8);
    }
    stream.read(&mut buffer).unwrap();
    let payload = std::str::from_utf8(&buffer).unwrap().to_string();
    //println!("{}", payload);

    return payload;
}

fn main() {
    let sockpath = match env::var("SWAYSOCK") {
        Ok(sockpath) => sockpath,
        Err(e) => panic!("{}", e)
    };
    let mut stream = UnixStream::connect(&sockpath).unwrap();

    let ipc_subscribe_cmd = sway_ipc_subscribe();
    stream.write_all(&ipc_subscribe_cmd).unwrap();

    /* get first success message*/
    let _ = get_next_message (&mut stream);

    /* setup aw_client and create buckets */
    let aw_client = aw_client_rust::AwClient::new("127.0.0.1", "5600", "aw-watcher-sway");
    let window_bucket = format!("aw-watcher-window_{}", aw_client.hostname);
    aw_client.create_bucket(&window_bucket, "currentwindow").unwrap();
    let afk_bucket = format!("aw-watcher-afk_{}", aw_client.hostname);
    aw_client.create_bucket(&afk_bucket, "afkstatus").unwrap();

    let mut prev_event : Option<aw_client_rust::Event> = None;
    loop {
        let payload = get_next_message (&mut stream);
        let data: Value = serde_json::from_str(&payload).unwrap();

        if let Value::Bool(focused) = data["container"]["focused"] {
            if focused {
                let appname_raw = &data["container"]["window_properties"]["class"].to_string();
                let title_raw = &data["container"]["name"].to_string();
                // remove "" from appname and title
                let appname = appname_raw[1..appname_raw.len()-1].to_string();
                let title = title_raw[1..title_raw.len()-1].to_string();

                let mut data = Map::new();
                data.insert("app".to_string(), Value::String(appname));
                data.insert("title".to_string(), Value::String(title));

                if let Some(e) = prev_event {
                    let mut e = e.clone();
                    let now = Utc::now();
                    let prev_timestamp = DateTime::parse_from_rfc3339(&e.timestamp).unwrap();
                    let pulsetime = now.timestamp_millis() - prev_timestamp.timestamp_millis() + 1000;
                    e.timestamp = now.to_rfc3339();
                    println!("{}s - {}: {}", &pulsetime/1000, &e.data["app"], &e.data["title"]);
                    match aw_client.heartbeat(&window_bucket, &e, pulsetime as f64) {
                        Ok(_) => (),
                        Err(e) => println!("{:?}", e),
                    };
                }

                let event = aw_client_rust::Event {
                    timestamp: Utc::now().to_rfc3339(),
                    duration: 0.0,
                    id: None,
                    data: data,
                };
                match aw_client.heartbeat(&window_bucket, &event, 0.0) {
                    Ok(_) => (),
                    Err(e) => println!("{:?}", e),
                };
                prev_event = Some(event);

                let mut data = Map::new();
                data.insert("status".to_string(), Value::String("not-afk".to_string()));
                let afk_event = aw_client_rust::Event {
                    timestamp: Utc::now().to_rfc3339(),
                    duration: 0.0,
                    id: None,
                    data: data,
                };
                match aw_client.heartbeat(&afk_bucket, &afk_event, 120.0) {
                    Ok(_) => (),
                    Err(e) => println!("{:?}", e),
                };
            }
        }
    }
}

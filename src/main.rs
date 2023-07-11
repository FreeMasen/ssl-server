use std::{collections::VecDeque, fmt::Display, ops::Range, sync::Mutex};

use actix_web::{
    body::{self, BoxBody},
    middleware,
    web::{self, Bytes, Data, Json, Path, Query},
    App, Error, HttpRequest, HttpResponse, HttpServer,
};
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
use rand::rngs::ThreadRng;
use serde::{Deserialize, Serialize};

/// simple handle
async fn index(req: HttpRequest) -> Result<HttpResponse, Error> {
    println!("{req:?}");
    Ok(HttpResponse::Ok().body(vec![9, 3, 2, 5, 6, 7, 8, 6, 5, 43]))
}

async fn sse(_req: HttpRequest) -> Result<HttpResponse, Error> {
    use futures::StreamExt;
    let stream = async_stream::stream! {
        let mut ct = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let mut message = mousse::ServerSentEvent::builder();
            message = message.comment("");
            let s = format!("{{ \"count\": {ct} }}");
            if ct % 10 == 0 {
                message = message.data(&s);
            }
            ct += 1;
            yield message.build().to_string()
        }
    };
    Ok(HttpResponse::Ok().body(BoxBody::new(body::BodyStream::new(
        stream.map(|s| Result::<_, Error>::Ok(Bytes::copy_from_slice(s.as_bytes()))),
    ))))
}

#[actix_web::get("/{count}")]
async fn counter(count: Path<u64>) -> Result<String, Error> {
    println!("GET {}", count);
    tokio::time::sleep(std::time::Duration::from_secs(*count)).await;
    Ok(format!("hello world ({})!", count))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Parrot {
    name: String,
    age: u8,
}

#[actix_web::post("/parrot")]
async fn parrot(who: Json<Parrot>) -> Result<HttpResponse, Error> {
    let ret = serde_json::to_string_pretty(&serde_json::json!({
        "message": format!("{} wants a cracker", who.name),
    }))?;

    let iter: Vec<Result<_, Error>> = ret
        .split_whitespace()
        .map(|s| {
            Ok(actix_web::web::Bytes::copy_from_slice(
                s.to_string().as_bytes(),
            ))
        })
        .collect();
    Ok(HttpResponse::Ok().streaming(futures::stream::iter(iter)))
}

#[derive(Debug, Clone, Deserialize)]
pub struct LargeQuery {
    sleep_between_chunks_ms: Option<u64>,
    max_chunk_size: Option<u16>,
    min_chunk_size: Option<u16>,
    max_chunks: Option<u16>,
    min_chunks: Option<u16>,
}

fn check_range<T>(lower: T, upper: T, name: &'static str) -> Result<Range<T>, Error>
where
    T: Display + PartialOrd,
{
    if lower >= upper {
        return Err(actix_web::error::ErrorInternalServerError(format!(
            "Invalid {name} params: {lower}..{upper} is an invalid range"
        )));
    }
    Ok(lower..upper)
}

struct CharIterator {
    ch: char
}

impl CharIterator {
    const CAP_A: char = 'A';
    const CAP_Z: char = 'Z';
    const LOW_A: char = 'a';
    const LOW_Z: char = 'z';
    pub fn next_string(&mut self, size: usize) -> String {
        let ret = String::from(self.ch).repeat(size);
        if self.ch == Self::LOW_Z {
            self.ch = Self::CAP_A;
        } else if self.ch == Self::CAP_Z {
            self.ch = Self::LOW_A;
        } else {
            self.ch = (self.ch as u8 + 1) as char;
        }
        ret
    }
}

impl Default for CharIterator {
    fn default() -> Self {
        Self {
            ch: Self::LOW_A
        }
    }
}

#[actix_web::get("/large")]
async fn large(
    query: Query<LargeQuery>,
    rng: Data<Mutex<ThreadRng>>,
) -> Result<HttpResponse, Error> {
    use rand::Rng;
    let LargeQuery {
        sleep_between_chunks_ms,
        max_chunk_size,
        min_chunk_size,
        max_chunks,
        min_chunks,
    } = dbg!(query.into_inner());
    let mut rng = rng.lock().map_err(|e| {
        actix_web::error::ErrorInternalServerError(format!("Error locking rng: {e}"))
    })?;
    let chunk_count_range =
        check_range(min_chunks.unwrap_or(5), max_chunks.unwrap_or(50), "chunks")?;
    let chunk_size_range = check_range(
        min_chunk_size.unwrap_or(50) as usize,
        max_chunk_size.unwrap_or(10_000) as usize,
        "chunk size",
    )?;
    let chunk_count: u16 = rng.gen_range(chunk_count_range);
    let mut ch = CharIterator::default();
    let mut iter: VecDeque<Result<_, Error>> = (0..chunk_count)
        .into_iter()
        .map(move |_| {
            let chunk_size: usize = rng.gen_range(chunk_size_range.clone());
            Ok(actix_web::web::Bytes::copy_from_slice(
                ch.next_string(chunk_size as usize).as_bytes(),
            ))
        })
        .collect();
    let stream = async_stream::stream! {
        while let Some(chunk) = iter.pop_front() {
            if let Some(ms) = sleep_between_chunks_ms {
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            }
            if let Ok(chunk) = &chunk {
                log::debug!("sending {0} ({0:x}) [{1}] byts in chunk", chunk.len(), chunk.get(0).map(|ch| *ch as char).unwrap_or(' '));
            }
            yield chunk
        }
    };
    Ok(HttpResponse::Ok().streaming(stream))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    println!("Started http server: 127.0.0.1:3030");

    // load TLS keys
    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    builder
        .set_private_key_file("key.rsa", SslFiletype::PEM)
        .unwrap();
    builder.set_certificate_chain_file("cert.pem").unwrap();

    HttpServer::new(|| {
        App::new()
            // enable logger
            .wrap(middleware::Logger::default())
            .app_data(Data::new(Mutex::new(rand::thread_rng())))
            // register simple handler, handle all methods
            .service(web::resource("/").to(index))
            .service(web::resource("/sse").to(sse))
            .service(large)
            .service(counter)
            .service(parrot)
    })
    .bind_openssl("0.0.0.0:3030", builder)?
    .run()
    .await
}

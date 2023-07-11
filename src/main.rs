use std::sync::Mutex;

use actix_web::{
    body::{self, BoxBody},
    middleware,
    web::{self, Bytes, Data, Json, Path},
    App, Error, HttpRequest, HttpResponse, HttpServer,
};
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
use rand::{rngs::ThreadRng, RngCore};
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

#[actix_web::get("/large")]
async fn large(rng: Data<Mutex<ThreadRng>>) -> Result<HttpResponse, Error> {
    use rand::Rng;
    let mut rng = rng.lock().map_err(|e| {
        actix_web::error::ErrorInternalServerError(format!("Error locking rng: {e}"))
    })?;
    let chunk_count: u16 = rng.gen_range(5..50);
    let iter: Vec<Result<_, Error>> = (0..chunk_count)
        .into_iter()
        .map(|_| {
            let chunk_size: u16 = rng.gen_range(50..10_000);
            let mut buf = vec![0u8; chunk_size as usize];
            rng.fill_bytes(&mut buf);
            Ok(actix_web::web::Bytes::copy_from_slice(&buf))
        })
        .collect();
    Ok(HttpResponse::Ok().streaming(futures::stream::iter(iter)))
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

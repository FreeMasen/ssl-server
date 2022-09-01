use std::io;

use actix_web::{middleware, web::{self, Bytes}, App, Error, HttpRequest, HttpResponse, HttpServer, body::{self, BoxBody}};
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};

/// simple handle
async fn index(req: HttpRequest) -> Result<HttpResponse, Error> {
    println!("{req:?}");
    Ok(HttpResponse::Ok()
        .body(vec![9,3,2,5,6,7,8,6,5,43]))
}

async fn sse(_req: HttpRequest) -> Result<HttpResponse, Error> {
    use futures::StreamExt;
    let stream = async_stream::stream!{
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
    Ok(HttpResponse::Ok()
        .body(BoxBody::new(body::BodyStream::new(stream.map(|s| Result::<_, Error>::Ok(Bytes::copy_from_slice(s.as_bytes())))))))
}

#[actix_web::main]
async fn main() -> io::Result<()> {
    env_logger::init();

    println!("Started http server: 127.0.0.1:8443");

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
            // register simple handler, handle all methods
            .service(web::resource("/").to(index))
            .service(web::resource("/sse").to(sse))
            
    })
    .bind_openssl("0.0.0.0:3030", builder)?
    .run()
    .await
}

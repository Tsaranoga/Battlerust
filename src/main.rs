
use battlerustlib::do_battle;

use std::env;
use std::fs;
use std::process;
#[cfg(feature = "server")]
use axum::{
    routing::{post},
    Router,
    response::IntoResponse,
    body::Bytes,
    http::StatusCode,
};
#[cfg(feature = "server")]
use tower_http::services::ServeDir;
#[cfg(feature = "server")]
use std::panic;

#[cfg(feature = "server")]
#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() > 2 && args[1] == "serve" {
        #[cfg(feature = "server")]
        run_server(args[2].clone()).await;
    } else {
        run_normal();
    }
}
#[cfg(not(feature = "server"))]
fn main() {
        run_normal();
}


#[cfg(feature = "server")]
async fn run_server(port:String) {
    // build our app
    let app = Router::new()
    .route("/battle", post(safe_battle))
    .fallback_service(ServeDir::new("static"));

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .unwrap();

    println!("Server running on http://127.0.0.1:{}", port);

    axum::serve(listener, app).await.unwrap();
}

#[cfg(feature = "server")]
async fn safe_battle(body: Bytes) -> impl IntoResponse {
    // Step 1: parse input safely
    let input = match String::from_utf8(body.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid UTF-8".to_string());
        }
    };

    // Step 2: catch panics from battle()
    let result = panic::catch_unwind(|| do_battle(&input, true));

    match result {
        Ok(output) => (StatusCode::OK, output),
        Err(err) => {
            // Optional: log panic info
            if let Some(msg) = err.downcast_ref::<&str>() {
                eprintln!("Panic: {}", msg);
            } else if let Some(msg) = err.downcast_ref::<String>() {
                eprintln!("Panic: {}", msg);
            } else {
                eprintln!("Unknown panic");
            }

            (StatusCode::INTERNAL_SERVER_ERROR, "{ \"error\": \"Battle crashed\" }".to_string())
        }
    }
}



//main function for local running
fn run_normal() {
    let json = r#"
  {
        "rounds":6,
        "attacker": [
            {
                "name": 123,
                "ships": [
                    {"shipid":12, "attack": 400.0, "hull": 2700.0, "shield": 50.0, "explode": 0.7, "amount": 1000000, "rapidfire":{"201":10,"11":5,"10":3} },
                    {"shipid":13, "attack": 1000.0, "hull": 6000.0, "shield": 50.0, "explode": 0.7, "amount": 1000000, "rapidfire":{"444":10} }
                    
                ]
            }
        ],
        "defender": [
                    {
                "name": 1234,
                "ships": [
                    {"shipid":10, "attack": 400.0, "hull": 2700.0, "shield": 50.0, "explode": 0.7, "amount": 1000000, "rapidfire":{"201":10} },
                    {"shipid":11, "attack": 1000.0, "hull": 6000.0, "shield": 50.0, "explode": 0.7, "amount": 1000000, "rapidfire":{"444":10} }
                    
                ]
            }
        ]
    }
    "#;
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        do_battle(json,true);
    }else{

    let filename = &args[1];
    let contents = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read file '{}': {}", filename, e);
            process::exit(1);
        }
    };
    let result=do_battle(&contents,true);
    let filenameout = &args[2];

    fs::write(filenameout, result)
        .expect("Failed to write to file");
    
    }
}

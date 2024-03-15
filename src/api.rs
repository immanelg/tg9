use anyhow::Result;
use grammers_client::{Client, Config, SignInError};
use grammers_session::Session;

use std::io::{self, BufRead};

static API_ID: &str = env!("TG9_API_ID");
static API_HASH: &str = env!("TG9_API_HASH");

fn read_prompt(prompt: &str) -> String {
    println!("{}", prompt);
    let mut line = String::new();
    let stdin = io::stdin();
    stdin.lock().read_line(&mut line).unwrap();
    line
}

pub async fn login() -> Result<Client> {
    let client = Client::connect(Config {
        session: Session::load_file_or_create("hello-world.session").unwrap(),
        api_id: API_ID.parse().expect("API ID should be valid i32"),
        api_hash: API_HASH.to_string(),
        params: Default::default(),
    })
    .await?;

    if !client.is_authorized().await.unwrap() {
        let phone = read_prompt("Phone number:");

        let token = client.request_login_code(&phone).await.unwrap();
        let code = read_prompt("Code: ");

        let _user = match client.sign_in(&token, &code).await {
            Ok(user) => {
                client
                    .session()
                    .save_to_file("hello-world.session")
                    .unwrap();
                println!("{:?}", user);
            }
            Err(SignInError::PasswordRequired(_token)) => {
                unimplemented!("Please provide a password");
            }
            Err(SignInError::SignUpRequired {
                terms_of_service: _tos,
            }) => {
                unimplemented!("Sign up required");
            }
            Err(err) => {
                println!("Failed to sign in as a user :(\n{}", err);
                return Err(err.into());
            }
        };
    }
    Ok(client)
}

// pub async fn start_client(rx: UnboundedReceiver, tx: UnboundedSender<>) {
//
// }

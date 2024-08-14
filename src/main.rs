use anyhow::{anyhow, Result};
use futures::future::try_join_all;
use reqwest::{header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT}, Client};
use serde::Deserialize;
use tokio::{sync::Mutex, task::JoinHandle};
use std::{collections::HashMap, error::Error, fmt::{Display, Formatter}, hash::Hash, sync::Arc};
use clap::Parser;


#[derive(Parser, Debug)]
#[command(version, about, long_about)]
struct Args {
    #[clap(short, long)]
    owner: String,
    #[clap(short, long)]
    repo: String,
}

struct GitHubUsers(HashMap<User, UserData>);

#[derive(Deserialize, Debug)]
struct PullRequest {
    pub number: u64,
}

#[derive(Clone, Deserialize, Debug)]
struct User {
    login: String,
}

#[derive(Deserialize, Debug)]
struct Review {
    user: User,
    state: String,
}

#[derive(Clone, Deserialize, Debug)]
struct UserData {
    approvals: u64,
    comments: u64,
}

impl UserData {
    fn new() -> UserData {
        UserData {
            approvals: 0,
            comments: 0,
        }
    }
    fn increment_reviews(&mut self) {
        self.approvals += 1;
    }
    fn increment_comments(&mut self) {
        self.comments += 1;
    }
}

impl PartialEq for User {
    fn eq(&self, other: &Self) -> bool {
        self.login == other.login
    }
}
impl Eq for User {}
impl Hash for User {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.login.hash(state);
    }
}
impl Display for GitHubUsers {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "[")?;
        for (user, data) in self.0.iter() {
            writeln!(f, "  {{\n    \"{}\":{{\n      \"Approvals\": {},\n      \"Comments\": {}\n    }}  \n  }},", user.login, data.approvals, data.comments)?;
        }
        writeln!(f, "]")?;
        Ok(())
    }
}
fn build_header_map(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", token)).unwrap());
    headers.insert(USER_AGENT, HeaderValue::from_static("rust-reqwest"));
    headers
}

async fn get_pull_requests(client: &Arc<Client>, token: &str, repo_owner: &str, repo_name: &str, page: &u64, per_page: &u64) -> Result<Vec<PullRequest>> {
    let pulls_url = format!("https://api.github.com/repos/{}/{}/pulls?state=closed&page={}&per_page={}", repo_owner, repo_name, page, per_page);
    println!("Processing Page {}", page);
    let response = client.get(&pulls_url)
        .headers(build_header_map(token))
        .send()
        .await?;
    if response.status().as_u16() != 200 {
        return Err(anyhow!("Failed to fetch pull requests due to: {:?}", response.text().await?));
    }
    let response_text = response.text().await?;
    let prs: Vec<PullRequest> = serde_json::from_str(&response_text).map_err(|e| anyhow!(e))?;
    Ok(prs)
}

async fn get_reviews(client: &Arc<Client>, token: &str, repo_owner: &str, repo_name: &str, pr: &u64) -> Result<Vec<Review>> {
    let pulls_url = format!("https://api.github.com/repos/{}/{}/pulls/{}/reviews", repo_owner, repo_name, pr);
    let response = client.get(&pulls_url)
        .headers(build_header_map(token))
        .send()
        .await?;
    if response.status().as_u16() != 200 {
        return Err(anyhow!("Failed to fetch reviews due to: {:?}", response.text().await?));
    }
    let response_text = response.text().await?;
    let reviews: Vec<Review> = serde_json::from_str(&response_text).map_err(|e| anyhow!(e))?;
    Ok(reviews)
}

async fn handle_review(reviews: Vec<Review>, data: Arc<Mutex<GitHubUsers>>) {
    for review in reviews {
        let user = review.user;
        if !data.lock().await.0.contains_key(&user) {
            data.lock().await.0.insert(user.clone(), UserData::new());
        }
        let mut lock = data.lock().await;
        if review.state == "APPROVED" {
            (*lock).0.get_mut(&user).unwrap().increment_reviews();
        } else if review.state == "COMMENTED" {
            (*lock).0.get_mut(&user).unwrap().increment_comments()
        }
    }
}

async fn process_pull_requests(
    client: Arc<Client>,
    token: String,
    repo_owner: String,
    repo_name: String,
    page_number: u64,
    per_page: u64,
    data: Arc<Mutex<GitHubUsers>>,
    semaphore: Arc<tokio::sync::Semaphore>,
) -> Result<(), Box<dyn Error>> {
    let _permit = semaphore.acquire().await.unwrap();
    match get_pull_requests(&client, &token, &repo_owner, &repo_name, &page_number, &per_page).await {
        Ok(pull_requests) => {
            let review_tasks: Vec<_> = pull_requests.into_iter().map(|pr| {
                let client = Arc::clone(&client);
                let token = token.clone();
                let data = Arc::clone(&data);
                let repo_owner = repo_owner.clone();
                let repo_name = repo_name.clone();
                let semaphore = Arc::clone(&semaphore);
                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();
                    match get_reviews(&client, &token, &repo_owner, &repo_name, &pr.number).await {
                        Ok(reviews) => handle_review(reviews, data).await,
                        Err(e) => eprintln!("Error fetching reviews for PR #{}: {:?}", pr.number, e),
                    }
                })
            }).collect();
            try_join_all(review_tasks).await?;
        },
        Err(e) => eprintln!("Error fetching pull requests for page {}: {:?}", page_number, e),
    }
    Ok(())
}

fn handle_pull_requests(
    client: Arc<Client>,
    token: String,
    repo_owner: String,
    repo_name: String,
    page_number: u64,
    per_page: u64,
    data: Arc<Mutex<GitHubUsers>>,
    semaphore: Arc<tokio::sync::Semaphore>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(e) = process_pull_requests(client, token, repo_owner, repo_name, page_number, per_page, data, semaphore).await {
            eprintln!("Error processing pull requests: {:?}", e);
        }
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let github_token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN not set");
    let args: Args = Args::parse();
    let repo_owner = args.owner;
    let repo_name = args.repo;

    let data: GitHubUsers = GitHubUsers(HashMap::new());
    let shared_data = Arc::new(Mutex::new(data));

    let client = reqwest::Client::new();
    let shared_client = Arc::new(client);
    let pr_count = get_pull_requests(&shared_client, &github_token, &repo_owner, &repo_name, &1, &1).await?;
    let per_page = 30;
    let number_of_prs = &pr_count[0].number;
    let number_of_pages = (number_of_prs / per_page) + 2;
    let semaphore = Arc::new(tokio::sync::Semaphore::new(5));
    println!("Number of PRs: {}", number_of_prs);
    println!("Number of pages: {}", number_of_pages);
    for page_number in 1..number_of_pages {
        if page_number % 5 == 0 {
            println!("Sleeping for 30 seconds to avoid rate limiting...");
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        }
        let mut tasks = Vec::new();
        let client = Arc::clone(&shared_client);
        let token = github_token.clone();
        let data = Arc::clone(&shared_data);
        let semaphore = Arc::clone(&semaphore);
        tasks.push(handle_pull_requests(client, token, repo_owner.to_owned(), repo_name.to_owned(), page_number, per_page, data, semaphore));
        for task in tasks {
            if let Err(e) = task.await {
                eprintln!("Error awaiting pull request task: {:?}", e);
        }
    }
    }
    println!("{}", shared_data.lock().await);
    Ok(())
}

use reqwest::{header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT}, Client};
use serde::Deserialize;
use tokio::sync::Mutex;
use std::{collections::HashMap, error::Error, fmt::{Display, Formatter}, hash::Hash, sync::Arc};


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
        for (user, data) in self.0.iter() {
            writeln!(f, "{}:\n  Approvals: {},\n  Comments: {}", user.login, data.approvals, data.comments)?;
        }
        Ok(())
    }
}
fn build_header_map(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", token)).unwrap());
    headers.insert(USER_AGENT, HeaderValue::from_static("rust-reqwest"));
    headers
}

async fn get_pull_requests(client: &Arc<Client>, token: &str, repo_owner: &str, repo_name: &str, page: &u64, per_page: &u64) -> Result<Vec<PullRequest>, reqwest::Error> {
    let pulls_url = format!("https://api.github.com/repos/{}/{}/pulls?state=closed&page={}&per_page={}", repo_owner, repo_name, page, per_page);
    client.get(&pulls_url)
        .headers(build_header_map(token))
        .send()
        .await?
        .json()
        .await
}

async fn get_reviews(client: &Arc<Client>, token: &str, repo_owner: &str, repo_name: &str, pr: &u64) -> Result<Vec<Review>, reqwest::Error> {
    let pulls_url = format!("https://api.github.com/repos/{}/{}/pulls/{}/reviews", repo_owner, repo_name, pr);
    client.get(&pulls_url)
        .headers(build_header_map(token))
        .send()
        .await?
        .json()
        .await
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

async fn handle_pull_requests() {
    // TODO
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let github_token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN not set");
    let repo_owner = "icd-tech";
    let repo_name = "reporting";

    let data: GitHubUsers = GitHubUsers(HashMap::new());
    let shared_data = Arc::new(Mutex::new(data));

    let client = reqwest::Client::new();
    let shared_client = Arc::new(client);
    let pr_count = get_pull_requests(&shared_client, &github_token, repo_owner, repo_name, &1, &1).await?;
    let per_page = 30;
    let number_of_prs = &pr_count[0].number;
    let number_of_pages = (number_of_prs / per_page) + 2;
    for page_number in 1..number_of_pages {
        let mut tasks = Vec::new();
        let client = Arc::clone(&shared_client);
        let token = github_token.clone();
        let data = Arc::clone(&shared_data);
        tasks.push(tokio::spawn(async move {
            let pull_requests = get_pull_requests(&client, &token, repo_owner, repo_name, &page_number, &per_page).await;
            let mut review_tasks = Vec::new();
            for pr in pull_requests.unwrap() {
                let client = Arc::clone(&client);
                let token = token.clone();
                let data = Arc::clone(&data);
                review_tasks.push(tokio::spawn(async move {
                    let review = get_reviews(&client, &token, repo_owner, repo_name, &pr.number).await.unwrap();
                    handle_review(review, data.clone()).await;
                }));
            }
            for task in review_tasks {
                task.await.unwrap();
            }
        }));
        for task in tasks {
            task.await?;
        }
    }
    println!("{}", shared_data.lock().await);
    Ok(())
}

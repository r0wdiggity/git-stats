use anyhow::Result;
use chrono::prelude::*;
use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    fmt::{Display, Formatter},
    sync::Arc,
};
use tokio::task::JoinSet;

struct GitHubUsers(HashMap<String, UserStats>);

struct ScoredUser(Vec<(String, UserStats)>);

impl GitHubUsers {
    fn finalize(&mut self, weight: &u64) -> ScoredUser {
        let mut v = Vec::new();
        for (user, stats) in self.0.iter() {
            let mut stats = stats.clone();
            let score = (stats.approvals * weight) + (stats.comments * weight) + (stats.requested_changes * 2 * weight) + stats.additions + (stats.deletions * (weight / 10) );
            stats.score = score;
            v.push((user.clone(), stats.clone()));
        }
        v.sort_by(|a, b| {
            b.1.score.cmp(&a.1.score)
        });
        ScoredUser(v)
    }
}


impl Display for ScoredUser {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "[")?;
        for (user, data) in self.0.iter() {
            writeln!(
                f,
                "  {{
\"{}\":{{
    \"Score\": {},
    \"Approvals\": {},
    \"Comments\": {}
    \"Requested Changes\": {},
    \"Pull Requests\": {},
    \"Additions\": {},
    \"Deletions\": {},
    \"Changed Files\": {},
  }}
}},",
                user,
                data.score,
                data.approvals,
                data.comments,
                data.requested_changes,
                data.pull_requests,
                data.additions,
                data.deletions,
                data.changed_files
            )?;
        }
        writeln!(f, "]")?;
        Ok(())
    }
}

#[derive(Parser, Debug)]
#[command(version, about, long_about)]
struct Args {
    #[arg(short, long)]
    owner: String,
    #[arg(short, long)]
    #[arg(value_delimiter(','))]
    repos: Option<Vec<String>>,
    #[arg(short, long)]
    #[arg(value_parser=parse_date)]
    date: Option<NaiveDate>,
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| anyhow::anyhow!(e))
}

#[derive(Debug, Serialize, Deserialize)]
struct OrganizationResponse {
    data: OrgData,
}

impl OrganizationResponse {
    fn has_next_page(&self) -> bool {
        self.data.organization.repositories.page_info.has_next_page
    }

    fn next_cursor(&self) -> String {
        self.data
            .organization
            .repositories
            .page_info
            .end_cursor
            .clone()
    }

    fn repositories(&self) -> Vec<String> {
        self.data
            .organization
            .repositories
            .edges
            .iter()
            .map(|edge| edge.node.name.clone())
            .collect()
    }

    fn extend(&mut self, other: OrganizationResponse) {
        self.data
            .organization
            .repositories
            .edges
            .extend(other.data.organization.repositories.edges);
        self.data.organization.repositories.page_info =
            other.data.organization.repositories.page_info;
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct OrgData {
    organization: Organization,
}

#[derive(Debug, Serialize, Deserialize)]
struct Organization {
    repositories: Repositories,
}

#[derive(Debug, Serialize, Deserialize)]
struct Repositories {
    edges: Vec<RepositoryEdge>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Debug, Serialize, Deserialize)]
struct RepositoryEdge {
    node: RepositoryNode,
}

#[derive(Debug, Serialize, Deserialize)]
struct RepositoryNode {
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RepositoryResponse {
    data: Data,
}

impl RepositoryResponse {
    fn empty() -> RepositoryResponse {
        RepositoryResponse {
            data: Data {
                repository: Repository {
                    pull_requests: PullRequests {
                        nodes: vec![],
                        page_info: PageInfo {
                            end_cursor: "".to_string(),
                            has_next_page: false,
                        },
                    },
                },
            },
        }
    }

    fn has_next_page(&self, max_date: Option<NaiveDate>) -> bool {
        let in_window = if let Some(max_date) = max_date {
            match self.data.repository.pull_requests.nodes.last() {
                Some(last) => last.merged_at.date_naive() > max_date,
                None => true,
            }
        } else {
            true
        };
        in_window && self.data.repository.pull_requests.page_info.has_next_page
    }

    fn next_cursor(&self) -> String {
        self.data
            .repository
            .pull_requests
            .page_info
            .end_cursor
            .clone()
    }

    fn extend(&mut self, other: RepositoryResponse) {
        self.data
            .repository
            .pull_requests
            .nodes
            .extend(other.data.repository.pull_requests.nodes);
        self.data.repository.pull_requests.page_info =
            other.data.repository.pull_requests.page_info;
    }

    fn trim(&mut self, max_date: Option<NaiveDate>) {
        if let Some(max_date) = max_date {
            self.data
                .repository
                .pull_requests
                .nodes
                .retain(|pr| pr.merged_at.date_naive() > max_date);
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Data {
    repository: Repository,
}

#[derive(Debug, Serialize, Deserialize)]
struct Repository {
    #[serde(rename = "pullRequests")]
    pull_requests: PullRequests,
}

#[derive(Debug, Serialize, Deserialize)]
struct PullRequests {
    nodes: Vec<PullRequest>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Debug, Serialize, Deserialize)]
struct PageInfo {
    #[serde(rename = "endCursor")]
    #[serde(deserialize_with = "default_on_null")]
    end_cursor: String,
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct PullRequest {
    reviews: Reviews,
    comments: Comments,
    #[serde(rename = "mergedAt")]
    merged_at: DateTime<Utc>,
    additions: u64,
    deletions: u64,
    #[serde(rename = "changedFiles")]
    changed_files: u64,
    #[serde(deserialize_with = "default_on_null")]
    author: User,
}

#[derive(Debug, Serialize, Deserialize)]
struct Reviews {
    nodes: Vec<Review>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Review {
    #[serde(deserialize_with = "default_on_null")]
    author: User,
    state: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Comments {
    nodes: Vec<Comment>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Comment {
    #[serde(deserialize_with = "default_on_null")]
    author: User,
}

#[derive(Debug, Serialize, Deserialize)]
struct User {
    login: String,
}

impl Default for User {
    fn default() -> User {
        User {
            login: "Unknown".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct UserStats {
    approvals: u64,
    requested_changes: u64,
    comments: u64,
    pull_requests: u64,
    additions: u64,
    deletions: u64,
    changed_files: u64,
    score: u64,
}

impl UserStats {
    fn new() -> UserStats {
        UserStats {
            approvals: 0,
            requested_changes: 0,
            comments: 0,
            pull_requests: 0,
            additions: 0,
            deletions: 0,
            changed_files: 0,
            score: 0,
        }
    }
}

fn default_on_null<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + Default,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(|x: Option<T>| x.unwrap_or_default())
}

async fn make_request(client: &Client, token: &str, query: &str) -> Result<String> {
    client
        .post("https://api.github.com/graphql")
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "rust-github-stats")
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await?
        .text()
        .await
        .map_err(|e| anyhow::anyhow!(e))
}

async fn get_repositories(
    client: &Client,
    token: &str,
    owner: &str,
    after: &str,
) -> Result<OrganizationResponse> {
    let query = format!(
        r#"
        query {{
          organization(login: "{}") {{
            repositories(first: 100, after: {}) {{
              edges {{
                node {{
                  name
                }}
              }}
              pageInfo {{
                endCursor
                hasNextPage
              }}
            }}
          }}
        }}
        "#,
        owner, after
    );

    let raw_resp = make_request(client, token, &query).await?;
    serde_json::from_str(&raw_resp).map_err(|e| anyhow::anyhow!(e))
}

async fn get_stats(
    client: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    after: &str,
) -> Result<RepositoryResponse> {
    let query = format!(
        r#"
        query {{
            repository(owner: "{}", name: "{}") {{
                pullRequests(first: 100, after: {}, states: MERGED, orderBy: {{field: CREATED_AT, direction: DESC}}) {{
                    nodes {{
                        mergedAt
                        additions
                        deletions
                        changedFiles
                        author {{
                            login
                        }}
                        reviews(first: 100) {{
                            nodes {{
                                author {{
                                    login
                                }}
                                state
                            }}
                        }}
                        comments(first: 100) {{
                            nodes {{
                                author {{
                                    login
                                }}
                            }}
                        }}
                    }}
                   pageInfo {{
                        endCursor
                        hasNextPage
                   }}
                }}
            }}
        }}
        "#,
        owner, repo, after
    );
    let raw_resp = make_request(client, token, &query).await?;
    match serde_json::from_str(&raw_resp).map_err(|e| anyhow::anyhow!(e)) {
        Ok(resp) => Ok(resp),
        Err(e) => {
            println!("Error: {}", e);
            println!("Bad Response: {}", raw_resp);
            Ok(RepositoryResponse::empty())
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let github_token = env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN not set");
    let args: Args = Args::parse();

    let owner = args.owner;
    let repos = args.repos;
    let date = args.date;

    println!("Fetching statistics for Owner: {}, Date: {:?}", owner, date);

    let client = Client::new();
    let shared_client = Arc::new(client);

    let repositories = match repos {
        Some(repos) => repos,
        None => {
            let mut repositories =
                get_repositories(&shared_client, &github_token, &owner, "null").await?;
            while repositories.has_next_page() {
                let cursor = format!("\"{}\"", repositories.next_cursor());
                let next_page =
                    get_repositories(&shared_client, &github_token, &owner, &cursor).await?;
                repositories.extend(next_page);
            }
            repositories.repositories()
        }
    };

    let semaphore = Arc::new(tokio::sync::Semaphore::new(5));
    let mut join_handles = JoinSet::new();
    for (i, repo) in repositories.into_iter().enumerate() {
        println!("Processing repo: {}", repo);
        // if i % 5 == 0 && i != 0 {
        //     println!("Sleeping for 10 seconds to avoid rate limiting");
        //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        // }
        if i % 10 == 0 && i != 0 {
            println!("Sleeping for 10 seconds to avoid rate limiting");
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        }
        let client = Arc::clone(&shared_client);
        let github_token = github_token.clone();
        let owner = owner.clone();
        let semaphore = Arc::clone(&semaphore);
        join_handles.spawn(async move {
            let _permit = semaphore.acquire().await?;
            let mut stats = get_stats(&client, &github_token, &owner, &repo, "null").await?;
            while stats.has_next_page(date) {
                let cursor = format!("\"{}\"", stats.next_cursor());
                let next_resp = get_stats(&client, &github_token, &owner, &repo, &cursor).await?;
                stats.extend(next_resp);
            }
            stats.trim(date);
            Ok(stats)
        });
    }
    let mut user_stats: GitHubUsers = GitHubUsers(HashMap::new());
    let mut loc: u64 = 0;
    let mut prs: u64 = 0;
    while let Some(result) = join_handles.join_next().await {
        let handle_result: Result<RepositoryResponse> = result?;
        let stats = handle_result?;

        for pr in stats.data.repository.pull_requests.nodes {
            let stats = user_stats
                .0
                .entry(pr.author.login)
                .or_insert(UserStats::new());
            stats.additions += pr.additions;
            stats.deletions += pr.deletions;
            stats.changed_files += pr.changed_files;
            stats.pull_requests += 1;
            prs += 1;
            loc += pr.additions + pr.deletions;
            for review in pr.reviews.nodes {
                let stats = user_stats
                    .0
                    .entry(review.author.login)
                    .or_insert(UserStats::new());
                if review.state == "APPROVED" {
                    stats.approvals += 1;
                } else if review.state == "COMMENTED" {
                    stats.comments += 1;
                } else if review.state == "CHANGES_REQUESTED" {
                    stats.requested_changes += 1;
                }
            }

            for comment in pr.comments.nodes {
                let stats = user_stats
                    .0
                    .entry(comment.author.login)
                    .or_insert(UserStats::new());
                stats.comments += 1;
            }
        }
    }

    let scale = loc / prs; // Average LOC per PR
    let scored = user_stats.finalize(&scale);
    println!("{}", scored);

    Ok(())
}

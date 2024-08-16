# git-stats
Github Statistics for the things you care about

### Get the Enterprise GH App Token
1. Get the Github App Id from your Github Enterprise Admin
2. Get the PEM file from your Github Enterprise Admin
3. Run teh following command to get the token
```bash
./access_token.sh <app_id> <pem_file>
```
Once you have the token, you need to set the environment variable
```bash
export GITHUB_TOKEN=<token>
```

This can all be done in one command:
```bash
export GITHUB_TOKEN=$(./access_token.sh <app_id> <pem_file> | awk '{print $2}')
```

### Run the Program
1. To run the program from this repo run the following command
```bash
cargo run -- <args>
```

Running the help command will show the arguments:
```bash
cargo run -- --help
```

The arguments are as follows:
```bash
Usage: git-stats [OPTIONS] --owner <OWNER>

Options:
  -o, --owner <OWNER>  
  -r, --repos <REPOS>  
  -d, --date <DATE>    
  -h, --help           Print help
  -V, --version        Print version
```

Repos can be a single repository, or a comma separated list of repositories. The date is in the format of `YYYY-MM-DD`.
Repos is optional, and if not provided, the program will default to all repositories in the organization.

### Examples
*get 1 repo since beginning of the year*
```bash
cargo run -- -o icd-tech -d 2024-1-1 -r 2gP
```

*get multiple repos data since last year*
```bash
cargo run -- -o icd-tech -d 2023-8-16 -r reporting,2gP   
```

*entire org from inception*
```bash
cargo run -- -o icd-tech
```

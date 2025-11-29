pub mod model;
pub mod schema;

use crate::graphql::schema::BifrostSchema;
use async_graphql::Request;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::{RawQuery, State},
    response::{Html, IntoResponse, Response},
};

pub use schema::create_schema;

pub async fn graphql_post_handler(
    State(schema): State<BifrostSchema>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

/// Combined handler for GET requests - serves GraphiQL UI if no query param, otherwise executes GraphQL
pub async fn graphql_or_graphiql(
    State(schema): State<BifrostSchema>,
    raw_query: RawQuery,
) -> Response {
    // Check if there's a query string with a 'query' parameter
    if let Some(query_string) = raw_query.0 {
        // Parse query string manually to extract the query parameter
        let params: std::collections::HashMap<String, String> =
            query_string
                .split('&')
                .filter_map(|pair| {
                    let mut parts = pair.splitn(2, '=');
                    match (parts.next(), parts.next()) {
                        (Some(key), Some(value)) => {
                            Some((
                                urlencoding::decode(key).ok()?.into_owned(),
                                urlencoding::decode(value).ok()?.into_owned(),
                            ))
                        }
                        _ => None,
                    }
                })
                .collect();

        // If there's a 'query' parameter, execute the GraphQL request
        if let Some(query) = params.get("query") {
            let mut request = Request::new(query.clone());

            if let Some(vars) = params.get("variables") {
                if let Ok(variables) = serde_json::from_str(vars) {
                    request = request.variables(variables);
                }
            }

            if let Some(op_name) = params.get("operationName") {
                request = request.operation_name(op_name);
            }

            let response: GraphQLResponse = schema.execute(request).await.into();
            return response.into_response();
        }
    }

    // No valid query params, serve GraphiQL UI
    Html(custom_playground_source("/")).into_response()
}

/// Generate custom GraphQL Playground HTML with sample queries.
fn custom_playground_source(endpoint: &str) -> String {
    let default_query = r#"{
    # Welcome to the PsychonautWiki API!
    #
    # To learn more about individual fields,
    # keep 'ctrl' (Windows) or 'cmd' (macOS)
    # pressed and click the field name. This
    # will open the respective documentation
    # entry in a sidebar on the right.
    #
    # If you have any questions or found an
    # issue or any bug, don't hesitate to
    # contact Kenan (kenan@psy.is).
    #
    # Happy hacking!

    substances(query: "Armodafinil") {
        name

        # routes of administration
        roas {
            name

            dose {
                units
                threshold
                heavy
                common { min max }
                light { min max }
                strong { min max }
            }

            duration {
                afterglow { min max units }
                comeup { min max units }
                duration { min max units }
                offset { min max units }
                onset { min max units }
                peak { min max units }
                total { min max units }
            }

            bioavailability {
                min max
            }
        }

        # subjective effects
        effects {
            name url
        }
    }
}"#;

    format!(
        r##"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>PsychonautWiki API - GraphQL Playground</title>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/graphql-playground-react/build/static/css/index.css" />
    <link rel="shortcut icon" href="https://cdn.jsdelivr.net/npm/graphql-playground-react/build/favicon.png" />
    <script src="https://cdn.jsdelivr.net/npm/graphql-playground-react/build/static/js/middleware.js"></script>
</head>
<body>
    <div id="root">
        <style>
            body {{
                background-color: rgb(23, 42, 58);
                font-family: Open Sans, sans-serif;
                height: 90vh;
            }}
            #root {{
                height: 100%;
                width: 100%;
                display: flex;
                align-items: center;
                justify-content: center;
            }}
            .loading {{
                font-size: 32px;
                font-weight: 200;
                color: rgba(255, 255, 255, .6);
                margin-left: 28px;
            }}
            img {{
                width: 78px;
                height: 78px;
            }}
            .title {{
                font-weight: 400;
            }}
        </style>
        <img src="https://cdn.jsdelivr.net/npm/graphql-playground-react/build/logo.png" alt="" />
        <div class="loading">
            Loading <span class="title">PsychonautWiki API</span>
        </div>
    </div>
    <script>
        window.addEventListener('load', function() {{
            GraphQLPlayground.init(document.getElementById('root'), {{
                endpoint: '{endpoint}',
                settings: {{
                    'editor.theme': 'dark',
                    'editor.cursorShape': 'line',
                    'editor.reuseHeaders': true,
                    'tracing.hideTracingResponse': true,
                    'editor.fontSize': 14,
                    'editor.fontFamily': "'Source Code Pro', 'Consolas', 'Inconsolata', 'Droid Sans Mono', 'Monaco', monospace",
                    'request.credentials': 'omit',
                }},
                tabs: [
                    {{
                        endpoint: '{endpoint}',
                        query: `{query}`,
                        name: 'Sample Query'
                    }}
                ]
            }});
        }});
    </script>
</body>
</html>"##,
        endpoint = endpoint,
        query = default_query.replace('`', "\\`").replace("${", "${{")
    )
}

pub mod model;
pub mod schema;

use crate::graphql::schema::BifrostSchema;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::State,
    response::{Html, IntoResponse},
};

pub use schema::create_schema;

pub async fn graphql_handler(
    State(schema): State<BifrostSchema>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

/// Custom GraphQL Playground with sample queries matching the legacy Node.js implementation.
pub async fn graphiql() -> impl IntoResponse {
    Html(custom_playground_source("/"))
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

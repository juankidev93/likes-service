use axum::{
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
};

const OPENAPI_SPEC: &str = include_str!("../../openapi.yaml");

pub(crate) async fn openapi_spec() -> Response {
    let mut response = (StatusCode::OK, OPENAPI_SPEC).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/yaml"),
    );
    response
}

pub(crate) async fn swagger_ui() -> Html<&'static str> {
    Html(
        r##"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Social API Docs</title>
    <link
      rel="stylesheet"
      href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css"
    />
    <style>
      body {
        margin: 0;
        background: #faf8f3;
      }

      .topbar {
        display: none;
      }
    </style>
  </head>
  <body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
    <script>
      window.addEventListener("load", function () {
        window.SwaggerUIBundle({
          url: "/openapi.yaml",
          dom_id: "#swagger-ui",
          deepLinking: true,
          displayRequestDuration: true,
          persistAuthorization: true
        });
      });
    </script>
  </body>
</html>
"##,
    )
}

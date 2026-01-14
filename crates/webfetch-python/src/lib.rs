//! Python bindings for WebFetch
//!
//! This module exposes the WebFetch tool contract to Python.

// Allow false positive clippy warning from pyo3 macro expansion
#![allow(clippy::useless_conversion)]

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use webfetch::{FetchError, HttpMethod, Tool, ToolBuilder, WebFetchRequest, WebFetchResponse};

/// Convert FetchError to PyErr
fn to_py_err(e: FetchError) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// Python wrapper for WebFetchRequest
#[pyclass(name = "WebFetchRequest")]
#[derive(Clone)]
pub struct PyWebFetchRequest {
    inner: WebFetchRequest,
}

#[pymethods]
impl PyWebFetchRequest {
    /// Create a new request
    #[new]
    #[pyo3(signature = (url, method=None, as_markdown=None, as_text=None))]
    fn new(
        url: String,
        method: Option<String>,
        as_markdown: Option<bool>,
        as_text: Option<bool>,
    ) -> PyResult<Self> {
        let mut req = WebFetchRequest::new(url);

        if let Some(m) = method {
            req.method = Some(m.parse::<HttpMethod>().map_err(PyValueError::new_err)?);
        }

        req.as_markdown = as_markdown;
        req.as_text = as_text;

        Ok(Self { inner: req })
    }

    /// Get URL
    #[getter]
    fn url(&self) -> &str {
        &self.inner.url
    }

    /// Get method
    #[getter]
    fn method(&self) -> Option<String> {
        self.inner.method.map(|m| m.to_string())
    }

    /// Get as_markdown flag
    #[getter]
    fn as_markdown(&self) -> Option<bool> {
        self.inner.as_markdown
    }

    /// Get as_text flag
    #[getter]
    fn as_text(&self) -> Option<bool> {
        self.inner.as_text
    }

    /// Convert to JSON string
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Create from JSON string
    #[staticmethod]
    fn from_json(json: &str) -> PyResult<Self> {
        let inner: WebFetchRequest =
            serde_json::from_str(json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }
}

/// Python wrapper for WebFetchResponse
#[pyclass(name = "WebFetchResponse")]
#[derive(Clone)]
pub struct PyWebFetchResponse {
    inner: WebFetchResponse,
}

#[pymethods]
impl PyWebFetchResponse {
    #[getter]
    fn url(&self) -> &str {
        &self.inner.url
    }

    #[getter]
    fn status_code(&self) -> u16 {
        self.inner.status_code
    }

    #[getter]
    fn content_type(&self) -> Option<&str> {
        self.inner.content_type.as_deref()
    }

    #[getter]
    fn size(&self) -> Option<u64> {
        self.inner.size
    }

    #[getter]
    fn last_modified(&self) -> Option<&str> {
        self.inner.last_modified.as_deref()
    }

    #[getter]
    fn filename(&self) -> Option<&str> {
        self.inner.filename.as_deref()
    }

    #[getter]
    fn format(&self) -> Option<&str> {
        self.inner.format.as_deref()
    }

    #[getter]
    fn content(&self) -> Option<&str> {
        self.inner.content.as_deref()
    }

    #[getter]
    fn truncated(&self) -> Option<bool> {
        self.inner.truncated
    }

    #[getter]
    fn method(&self) -> Option<&str> {
        self.inner.method.as_deref()
    }

    #[getter]
    fn error(&self) -> Option<&str> {
        self.inner.error.as_deref()
    }

    /// Convert to JSON string
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn __repr__(&self) -> String {
        format!(
            "WebFetchResponse(url={:?}, status_code={})",
            self.inner.url, self.inner.status_code
        )
    }
}

/// Python wrapper for WebFetch Tool
#[pyclass(name = "WebFetchTool")]
pub struct PyWebFetchTool {
    inner: Tool,
    runtime: tokio::runtime::Runtime,
}

#[pymethods]
impl PyWebFetchTool {
    /// Create a new tool with default options
    #[new]
    #[pyo3(signature = (enable_markdown=true, enable_text=true, user_agent=None, allow_prefixes=None, block_prefixes=None))]
    fn new(
        enable_markdown: bool,
        enable_text: bool,
        user_agent: Option<String>,
        allow_prefixes: Option<Vec<String>>,
        block_prefixes: Option<Vec<String>>,
    ) -> PyResult<Self> {
        let mut builder = ToolBuilder::new()
            .enable_markdown(enable_markdown)
            .enable_text(enable_text);

        if let Some(ua) = user_agent {
            builder = builder.user_agent(ua);
        }

        if let Some(prefixes) = allow_prefixes {
            for prefix in prefixes {
                builder = builder.allow_prefix(prefix);
            }
        }

        if let Some(prefixes) = block_prefixes {
            for prefix in prefixes {
                builder = builder.block_prefix(prefix);
            }
        }

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| PyValueError::new_err(format!("Failed to create runtime: {}", e)))?;

        Ok(Self {
            inner: builder.build(),
            runtime,
        })
    }

    /// Get tool description
    fn description(&self) -> &'static str {
        self.inner.description()
    }

    /// Get system prompt
    fn system_prompt(&self) -> &'static str {
        self.inner.system_prompt()
    }

    /// Get full documentation (llmtxt)
    fn llmtxt(&self) -> &'static str {
        self.inner.llmtxt()
    }

    /// Get input schema as JSON string
    fn input_schema(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner.input_schema())
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Get output schema as JSON string
    fn output_schema(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner.output_schema())
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Execute a fetch request
    fn execute(&self, request: PyWebFetchRequest) -> PyResult<PyWebFetchResponse> {
        let result = self.runtime.block_on(self.inner.execute(request.inner));
        match result {
            Ok(response) => Ok(PyWebFetchResponse { inner: response }),
            Err(e) => Err(to_py_err(e)),
        }
    }

    /// Fetch a URL directly (convenience method)
    #[pyo3(signature = (url, method=None, as_markdown=None, as_text=None))]
    fn fetch(
        &self,
        url: String,
        method: Option<String>,
        as_markdown: Option<bool>,
        as_text: Option<bool>,
    ) -> PyResult<PyWebFetchResponse> {
        let request = PyWebFetchRequest::new(url, method, as_markdown, as_text)?;
        self.execute(request)
    }
}

/// Fetch a URL using default options (convenience function)
#[pyfunction]
#[pyo3(signature = (url, method=None, as_markdown=None, as_text=None))]
fn fetch(
    url: String,
    method: Option<String>,
    as_markdown: Option<bool>,
    as_text: Option<bool>,
) -> PyResult<PyWebFetchResponse> {
    let tool = PyWebFetchTool::new(true, true, None, None, None)?;
    tool.fetch(url, method, as_markdown, as_text)
}

/// Python module definition
#[pymodule]
fn webfetch_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyWebFetchRequest>()?;
    m.add_class::<PyWebFetchResponse>()?;
    m.add_class::<PyWebFetchTool>()?;
    m.add_function(wrap_pyfunction!(fetch, m)?)?;
    Ok(())
}

# TOS Conformance Test Harness Dockerfile

FROM python:3.11-slim

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy requirements first for better caching
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy harness code
COPY . .

# Create results directory
RUN mkdir -p /results

# Set environment defaults
ENV RUST_ENDPOINT=http://tos-rust:8080
ENV C_ENDPOINT=http://avatar-c:8080
ENV VECTOR_DIR=/vectors
ENV RESULT_DIR=/results
ENV LOG_LEVEL=INFO

# Run the test harness
ENTRYPOINT ["python", "runner.py"]
CMD ["--vectors=/vectors"]

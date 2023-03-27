REPOSITORY=$(cat repository.txt)
TAG=$(cat tag.txt)
TAG_LATEST="$TAG:latest"
aws ecr get-login-password --region us-east-2 | docker login --username AWS --password-stdin 879655802347.dkr.ecr.us-east-2.amazonaws.com/
docker build -t smithy-rs-compiletime-benchmark .

docker tag smithy-rs-compiletime-benchmark:latest 879655802347.dkr.ecr.us-east-2.amazonaws.com/smithy-rs-compiletime-benchmark:latest
docker push 879655802347.dkr.ecr.us-east-2.amazonaws.com/smithy-rs-compiletime-benchmark:latest

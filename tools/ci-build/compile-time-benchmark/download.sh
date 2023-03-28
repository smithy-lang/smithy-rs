https://raw.githubusercontent.com/awslabs/aws-sdk-rust/main/aws-models/batch.json
https://raw.githubusercontent.com/awslabs/aws-sdk-rust/main/aws-models/cloudcontrol.json 
https://raw.githubusercontent.com/awslabs/aws-sdk-rust/main/aws-models/ec2.json
https://raw.githubusercontent.com/awslabs/aws-sdk-rust/main/aws-models/resource-explorer-2.json
https://raw.githubusercontent.com/awslabs/aws-sdk-rust/main/aws-models/iam.json

list="batch cloudcontrol ec2 resource-explorer-2 iam"
for i in ${list[@]}; do
    curl -o ./$i.json https://raw.githubusercontent.com/awslabs/aws-sdk-rust/main/aws-models/$i.json
done;

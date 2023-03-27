git clone https://github.com/awslabs/smithy-rs.git
cd smithy-rs

./gradlew :aws:sdk:cargoCheck

PATH_TO_GENERATED_SDK="aws/sdk/build/aws-sdk/sdk"

for i in $(ls $PATH_TO_GENERATED_SDK); do
    cd $PATH_TO_GENERATED_SDK/$i
    if [[ ! $($PATH_TO_GENERATED_SDK == *"aws-"*) ]]; then
        # we need to do this to make sure that everything is downloaded 
        # and there are caches for the compilation
        cargo build 
        cargo build --release
        echo $PATH_TO_GENERATED_SDK/$i >> compiletime.txt
        time cargo build >> file.txt
        time cargo build --release >> file.txt
        echo "=======================================" >> compiletime.txt
    fi
done

python3 format_file_txt.py
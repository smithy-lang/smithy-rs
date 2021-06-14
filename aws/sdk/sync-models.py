import sys
import os
from pathlib import Path
import shutil

if len(sys.argv) != 2:
    print("Please provide the location of the aws-models repository as the first argument")
    sys.exit(1)

aws_models = sys.argv[1]

for model in os.listdir("aws-models"):
    if not model.endswith('.json'):
        continue
    model_name = model[:-len('.json')]
    source = Path(aws_models) / model_name / 'smithy' / 'model.json'
    if not source.exists():
        print(f'cannout find: {source}')
        sys.exit(1)
    shutil.copyfile(source, Path('aws-models') / model)

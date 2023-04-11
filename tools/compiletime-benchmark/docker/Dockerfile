FROM amazonlinux
# Comment
WORKDIR /root/
RUN yum -y install python3
RUN yum -y install python3-pip
RUN yum -y install unzip
RUN yum -y install python3-setuptools
RUN yum -y update
RUN curl "https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip" -o "awscliv2.zip" && unzip awscliv2.zip && ./aws/install && rm -r *
RUN pip3 install pandas 
RUN pip3 install polars 
RUN pip3 install boto3
RUN pip3 install zstd
RUN pip3 install pyarrow
RUN yum -y remove python3-pip python3-setuptools unzip
ADD ./files .
CMD python3 /root/main.sh
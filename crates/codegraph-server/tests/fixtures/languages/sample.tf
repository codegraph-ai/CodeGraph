variable "instance_type" {
  type    = string
  default = "t3.micro"
}

resource "aws_instance" "web" {
  ami           = "ami-12345"
  instance_type = var.instance_type

  tags = {
    Name = "web-server"
  }
}

module "vpc" {
  source = "./modules/vpc"
  cidr   = "10.0.0.0/16"
}

output "instance_ip" {
  value = aws_instance.web.public_ip
}

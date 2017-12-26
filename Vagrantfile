# -*- mode: ruby -*-
# vi: set ft=ruby :

Vagrant.configure("2") do |config|
  config.vm.box = "generic/freebsd11"
  config.vm.base_mac = "080027D14C66"
  config.vm.guest = :freebsd
  config.vm.synced_folder ".", "/vagrant", type: "rsync"
  config.ssh.shell = "sh"

  config.vm.provision "shell", inline: <<-SHELL
    sudo pkg install -y rust neovim
  SHELL
end

# Training apps in a lab

Deliberately vulnerable web applications, the targets of many labs, are not part of Isoloom's
catalogue: Isoloom stays a generic tool, and these belong with the labs that use them. A lab
names the application's prebuilt image itself, pinned by digest so every learner gets the same
build. Both run wherever a lab's machines are containers (on this computer, in a VM here, on a
server or in the cloud); an image can't be a VM, so a lab whose other machines are VMs can't use
them.

Add the machine to the lab's `isoloom.yml`, on one of its networks, with an address of its own.

## DVWA

[Damn Vulnerable Web Application](https://github.com/digininja/DVWA) (GPL-3.0), a PHP/MySQL
application in one image (Apache, PHP and MySQL inside). Version 1.9, amd64 only: on an Arm
computer it runs under emulation.

```yaml
machines:
  dvwa:
    os: debian@12
    networks: { lab: 10 }
    app:
      image: docker.io/vulnerables/web-dvwa@sha256:14c929ec1a99edb77326201f7f0fb431d1b3dd099c09ddeaf46312ab0ff53cfe
    ports: { http: { port: 80, publish: 8080 } }
checks:
  - name: DVWA serves its setup page
    http: http://dvwa:80/setup.php
    contains: DVWA
    wait: 2m
```

## OWASP Juice Shop

[OWASP Juice Shop](https://github.com/juice-shop/juice-shop) (MIT), a Node.js shop with the OWASP
Top Ten and more. Version 17.1.0, amd64 and arm64.

```yaml
machines:
  juice-shop:
    os: debian@12
    networks: { lab: 11 }
    app:
      image: docker.io/bkimminich/juice-shop@sha256:57170ba190744b9e22469d9ab5934946cd3e53e2e66b12e4c0c1019e1bea7a6b
    ports: { http: { port: 3000, publish: 3000 } }
checks:
  - name: Juice Shop answers
    http: http://juice-shop:3000/
    expect: 200
    wait: 2m
```

## A newer release

Look up the new tag's digest (`docker buildx imagetools inspect <image>:<tag>`), replace the
digest after `@sha256:`, and run the lab's checks: `isoloom validate` then a start from the
launcher. The `os:` line is nominal for an image (its own system runs); keep it as it is.

.PHONY: bully server

bully:
	$(error "Maybe don't just type `make` in random repositories?")

server:
	docker build . -f=server.dockerfile -t=frenetiq/caoq:bleeding

push: server
	docker push frenetiq/caoq:bleeding
